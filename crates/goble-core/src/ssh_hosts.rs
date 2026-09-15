//! The SSH connections this machine already knows, read from `~/.ssh`.
//!
//! A pure reader: goble never writes these files and never returns key material,
//! only the paths a config names and whether they exist. The parse is a pure
//! function over the file text, so it is tested against fixtures instead of a
//! real home directory. Design: `.agents/06-renderer/ui-spec/10-settings-tab.md`
//! (Decision 6).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// One concrete `Host` block, with the OpenSSH defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_files: Vec<String>,
    pub known: bool,
}

impl SshHost {
    /// The target a picker shows: `user@hostname:port`.
    pub fn target(&self) -> String {
        format!("{}@{}:{}", self.user, self.hostname, self.port)
    }
}

/// A key file the page can offer: an `id_*` file in `~/.ssh`, or one a `Host`
/// block names. Existence only — the file is never opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKeyFile {
    /// The name as shown to the user: the `IdentityFile` value, or the file name.
    pub name: String,
    /// Resolved path: `~/` expanded, a relative value taken under `~/.ssh`.
    pub path: PathBuf,
    pub exists: bool,
}

/// Why the page has nothing, or little, to show. Normal machine states, not
/// errors and not an empty page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshFinding {
    /// There is no `~/.ssh` directory at all.
    NoSshDir { path: PathBuf },
    /// `~/.ssh` exists but holds no `config`.
    NoConfig { path: PathBuf },
    /// A `config` exists that declares no concrete host.
    NoConcreteHost { path: PathBuf },
}

impl SshFinding {
    /// The path this finding is about.
    pub fn path(&self) -> &Path {
        match self {
            SshFinding::NoSshDir { path }
            | SshFinding::NoConfig { path }
            | SshFinding::NoConcreteHost { path } => path,
        }
    }

    /// The muted line the settings page draws.
    pub fn message(&self) -> String {
        match self {
            SshFinding::NoSshDir { path } => {
                format!("No SSH directory at {} — no connections are configured", path.display())
            }
            SshFinding::NoConfig { path } => {
                format!("No SSH config at {} — this machine names no connections", path.display())
            }
            SshFinding::NoConcreteHost { path } => format!(
                "{} declares no concrete host — only wildcard, negated or match rules",
                path.display()
            ),
        }
    }
}

/// Everything the Connections page reads out of `~/.ssh` for one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHosts {
    /// The `~/.ssh` directory that was read (whether or not it exists).
    pub ssh_dir: PathBuf,
    /// `~/.ssh/config`.
    pub config_path: PathBuf,
    /// Concrete hosts, sorted by alias.
    pub hosts: Vec<SshHost>,
    /// `id_*` files plus every `IdentityFile` a block names, sorted by name.
    pub key_files: Vec<SshKeyFile>,
    /// At most one, describing why the host list is empty.
    pub findings: Vec<SshFinding>,
    /// `known_hosts` entries whose names are hashed and therefore unrecoverable.
    pub hashed_known_hosts: usize,
}

/// The names `known_hosts` records, plus the count of hashed entries whose names
/// cannot be recovered from the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownHosts {
    plain: BTreeSet<String>,
    pub hashed: usize,
}

impl KnownHosts {
    /// Whether `name` is recorded in plain form. Host names compare
    /// case-insensitively.
    pub fn is_known(&self, name: &str) -> bool {
        self.plain.contains(&name.to_ascii_lowercase())
    }
}

/// Read the SSH state of the machine whose home directory is `home`.
///
/// `~/.ssh` is `home.join(".ssh")`: the caller passes the home directory, so the
/// read never depends on the process environment.
pub fn read_ssh_hosts(home: &Path) -> SshHosts {
    let ssh_dir = home.join(".ssh");
    let config_path = ssh_dir.join("config");
    let mut out = SshHosts {
        ssh_dir: ssh_dir.clone(),
        config_path: config_path.clone(),
        hosts: Vec::new(),
        key_files: Vec::new(),
        findings: Vec::new(),
        hashed_known_hosts: 0,
    };

    if !ssh_dir.is_dir() {
        out.findings.push(SshFinding::NoSshDir { path: ssh_dir });
        return out;
    }

    if !config_path.is_file() {
        out.findings.push(SshFinding::NoConfig { path: config_path });
    } else {
        let text = read_lossy(&config_path);
        out.hosts = parse_ssh_config(&text, &current_user());
        if out.hosts.is_empty() {
            out.findings.push(SshFinding::NoConcreteHost { path: config_path });
        }
    }

    let known = parse_known_hosts(&read_lossy(&ssh_dir.join("known_hosts")));
    out.hashed_known_hosts = known.hashed;
    for host in &mut out.hosts {
        // Either name counts: ssh records the name it connected under, which is
        // the alias when the block sets no HostName.
        host.known = known.is_known(&host.hostname) || known.is_known(&host.alias);
    }

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut names: Vec<String> = fs::read_dir(&ssh_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| !t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.starts_with("id_"))
        .collect();
    names.sort();
    for name in names {
        let path = ssh_dir.join(&name);
        if !seen.insert(path.clone()) {
            continue;
        }
        out.key_files.push(SshKeyFile {
            name,
            exists: path.is_file(),
            path,
        });
    }
    // What a block names is listed even when the file is missing: a block
    // pointing at a key that is not there is worth seeing.
    for host in &out.hosts {
        for value in &host.identity_files {
            let path = resolve_identity(home, &ssh_dir, value);
            if !seen.insert(path.clone()) {
                continue;
            }
            out.key_files.push(SshKeyFile {
                name: value.clone(),
                exists: path.is_file(),
                path,
            });
        }
    }
    out.key_files.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));
    out
}

/// Parse the concrete `Host` blocks of an `ssh_config(5)` file.
///
/// Pure over the text: `current_user` supplies the `User` fallback. `Include` is
/// not followed and `Match` blocks are skipped, so nothing is invented.
pub fn parse_ssh_config(text: &str, current_user: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut section = Section::None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(char::is_whitespace) {
            Some((key, value)) => (key, unquote(value.trim())),
            None => (line, ""),
        };
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                flush(&mut hosts, section.take_block(), current_user);
                let patterns: Vec<&str> = value.split_whitespace().collect();
                // Wildcards and `!` negations are rules about other connections,
                // not connections; listing them would be a lie.
                let concrete = !patterns.is_empty()
                    && patterns
                        .iter()
                        .all(|p| !p.contains('*') && !p.contains('?') && !p.starts_with('!'));
                section = if concrete {
                    Section::Block(HostBlock {
                        aliases: patterns.into_iter().map(str::to_string).collect(),
                        ..HostBlock::default()
                    })
                } else {
                    Section::Skipped
                };
            }
            "match" => {
                flush(&mut hosts, section.take_block(), current_user);
                section = Section::Skipped;
            }
            "hostname" | "user" | "port" | "identityfile" => {
                if let Section::Block(block) = &mut section {
                    match key.to_ascii_lowercase().as_str() {
                        "hostname" if !value.is_empty() => {
                            block.hostname.get_or_insert_with(|| value.to_string());
                        }
                        "user" if !value.is_empty() => {
                            block.user.get_or_insert_with(|| value.to_string());
                        }
                        "port" if block.port.is_none() => {
                            block.port = value.parse::<u16>().ok();
                        }
                        // More than one IdentityFile is legal; the first is used.
                        "identityfile" if !value.is_empty() => {
                            block.identity_files.push(value.to_string());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    flush(&mut hosts, section.take_block(), current_user);

    hosts.sort_by(|a, b| a.alias.cmp(&b.alias));
    hosts
}

/// Parse the plain host names of a `known_hosts` file. Hashed entries (`|1|…`)
/// are counted, never guessed.
pub fn parse_known_hosts(text: &str) -> KnownHosts {
    let mut out = KnownHosts::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(mut token) = fields.next() else {
            continue;
        };
        // `@cert-authority` / `@revoked` markers prefix the host list.
        if token.starts_with('@') {
            let Some(next) = fields.next() else {
                continue;
            };
            token = next;
        }
        if token.starts_with('|') {
            out.hashed += 1;
            continue;
        }
        for name in token.split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            // `[host]:port` is the non-default-port form.
            let name = name
                .strip_prefix('[')
                .and_then(|rest| rest.split(']').next())
                .unwrap_or(name);
            out.plain.insert(name.to_ascii_lowercase());
        }
    }
    out
}

#[derive(Default)]
struct HostBlock {
    aliases: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_files: Vec<String>,
}

enum Section {
    /// Before the first `Host` line: keyword lines there are global defaults.
    None,
    Block(HostBlock),
    /// A `Host *`-style rule or a `Match` block.
    Skipped,
}

impl Section {
    fn take_block(&mut self) -> Option<HostBlock> {
        match std::mem::replace(self, Section::None) {
            Section::Block(block) => Some(block),
            _ => None,
        }
    }
}

fn flush(hosts: &mut Vec<SshHost>, block: Option<HostBlock>, current_user: &str) {
    let Some(block) = block else {
        return;
    };
    for alias in block.aliases {
        hosts.push(SshHost {
            // A block with no HostName is reached under its alias, so the alias
            // is the honest hostname as well.
            hostname: block
                .hostname
                .clone()
                .unwrap_or_else(|| alias.clone()),
            user: block.user.clone().unwrap_or_else(|| current_user.to_string()),
            port: block.port.unwrap_or(22),
            identity_files: block.identity_files.clone(),
            alias,
            known: false,
        });
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

/// `~/` expands to the home directory; a bare name is taken under `~/.ssh`,
/// which is where the unqualified names configs usually write are looked up.
fn resolve_identity(home: &Path, ssh_dir: &Path, value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        return home.join(rest);
    }
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        ssh_dir.join(value)
    }
}

fn read_lossy(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// The `User` fallback for a block that sets none.
fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_BLOCKS: &str = "\
Host web
    HostName web.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_web

Host db
    HostName db.example.com
    User admin
    Port 5432
    IdentityFile ~/.ssh/id_db
";

    fn host(alias: &str) -> SshHost {
        parse_ssh_config(TWO_BLOCKS, "fallback")
            .into_iter()
            .find(|h| h.alias == alias)
            .expect("alias present")
    }

    /// A throwaway home directory, removed when the test ends so the user's real
    /// `~/.ssh` is never touched.
    struct TempHome(PathBuf);

    impl TempHome {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "goble-ssh-test-{}-{}",
                std::process::id(),
                test
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp home");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, rel: &str, contents: &str) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().expect("parent")).expect("fixture dir");
            fs::write(path, contents).expect("fixture file");
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn two_concrete_blocks() {
        let hosts = parse_ssh_config(TWO_BLOCKS, "fallback");
        assert_eq!(
            hosts,
            vec![
                SshHost {
                    alias: "db".into(),
                    hostname: "db.example.com".into(),
                    user: "admin".into(),
                    port: 5432,
                    identity_files: vec!["~/.ssh/id_db".into()],
                    known: false,
                },
                SshHost {
                    alias: "web".into(),
                    hostname: "web.example.com".into(),
                    user: "deploy".into(),
                    port: 2222,
                    identity_files: vec!["~/.ssh/id_web".into()],
                    known: false,
                },
            ]
        );
        assert_eq!(host("web").target(), "deploy@web.example.com:2222");
    }

    #[test]
    fn wildcard_star_block_is_not_a_host() {
        let hosts = parse_ssh_config("Host *\n    ServerAliveInterval 60\n", "me");
        assert!(hosts.is_empty());
    }

    #[test]
    fn negated_block_is_not_a_host() {
        let hosts = parse_ssh_config("Host !prod\n    User nobody\n", "me");
        assert!(hosts.is_empty());
    }

    #[test]
    fn wildcard_suffix_block_is_not_a_host() {
        let hosts = parse_ssh_config("Host *.internal\n    User ops\n", "me");
        assert!(hosts.is_empty());
    }

    #[test]
    fn block_without_hostname_or_port_uses_alias_and_defaults() {
        let hosts = parse_ssh_config("Host legacy\n    User alice\n", "me");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "legacy");
        assert_eq!(hosts[0].hostname, "legacy");
        assert_eq!(hosts[0].user, "alice");
        assert_eq!(hosts[0].port, 22);

        let bare = parse_ssh_config("Host bare\n", "current-user");
        assert_eq!(bare[0].hostname, "bare");
        assert_eq!(bare[0].user, "current-user");
        assert_eq!(bare[0].port, 22);
        assert!(bare[0].identity_files.is_empty());
    }

    #[test]
    fn two_identity_files_are_both_kept() {
        let hosts = parse_ssh_config(
            "Host two\n    HostName t.example.com\n    IdentityFile ~/.ssh/one\n    IdentityFile ~/.ssh/two\n",
            "me",
        );
        // Both are kept; the first is the one ssh uses.
        assert_eq!(hosts[0].identity_files, vec!["~/.ssh/one", "~/.ssh/two"]);
    }

    #[test]
    fn match_block_is_ignored() {
        let hosts = parse_ssh_config(
            "\
Host before
    HostName before.example.com
Match host *.internal
    HostName ignored.example.com
    User ignored
Host after
    HostName after.example.com
",
            "me",
        );
        assert_eq!(
            hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
            vec!["after", "before"]
        );
        let after = hosts.iter().find(|h| h.alias == "after").expect("after");
        assert_eq!(after.hostname, "after.example.com");
        // The `Match` block's keywords must not leak into the next block.
        assert_eq!(after.user, "me");
        assert!(!hosts.iter().any(|h| h.hostname == "ignored.example.com"));
    }

    #[test]
    fn include_is_not_followed() {
        let hosts = parse_ssh_config(
            "Include ~/.ssh/conf.d/*\n\nHost still-here\n    HostName s.example.com\n",
            "me",
        );
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "still-here");
    }

    #[test]
    fn second_hostname_inside_a_block_is_ignored() {
        let hosts = parse_ssh_config(
            "Host dup\n    HostName first.example.com\n    HostName second.example.com\n",
            "me",
        );
        assert_eq!(hosts[0].hostname, "first.example.com");
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let hosts = parse_ssh_config(
            "\
# a comment
Host commented

    # indented comment
    HostName c.example.com
    # User nope

",
            "me",
        );
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "c.example.com");
        assert_eq!(hosts[0].user, "me");
    }

    #[test]
    fn hosts_are_sorted_by_alias() {
        let hosts = parse_ssh_config(
            "\
Host zeta
    HostName z.example.com
Host alpha
    HostName a.example.com
Host mu
    HostName m.example.com
",
            "me",
        );
        assert_eq!(
            hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "mu", "zeta"]
        );
    }

    #[test]
    fn missing_ssh_dir_reports_a_finding() {
        let home = TempHome::new("missing-ssh-dir");
        let out = read_ssh_hosts(home.path());

        assert_eq!(out.ssh_dir, home.path().join(".ssh"));
        assert!(out.hosts.is_empty());
        assert!(out.key_files.is_empty());
        assert_eq!(out.hashed_known_hosts, 0);
        assert_eq!(
            out.findings,
            vec![SshFinding::NoSshDir {
                path: home.path().join(".ssh")
            }]
        );
        assert!(out.findings[0].message().contains(".ssh"));
    }

    #[test]
    fn ssh_dir_without_config_reports_a_finding() {
        let home = TempHome::new("no-config");
        home.write(".ssh/id_ed25519", "fixture key\n");
        let out = read_ssh_hosts(home.path());

        assert_eq!(
            out.findings,
            vec![SshFinding::NoConfig {
                path: home.path().join(".ssh/config")
            }]
        );
        assert!(out.findings[0].message().contains("config"));
        assert_eq!(
            out.key_files.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(),
            vec!["id_ed25519"]
        );
        assert!(out.key_files[0].exists);
    }

    #[test]
    fn config_with_no_concrete_host_reports_a_finding() {
        let home = TempHome::new("no-concrete-host");
        home.write(".ssh/config", "Host *\n    User root\n");
        let out = read_ssh_hosts(home.path());

        assert!(out.hosts.is_empty());
        assert_eq!(
            out.findings,
            vec![SshFinding::NoConcreteHost {
                path: home.path().join(".ssh/config")
            }]
        );
    }

    #[test]
    fn hashed_known_hosts_entries_are_only_a_count() {
        let home = TempHome::new("hashed-known-hosts");
        home.write(".ssh/config", "Host box\n    HostName box.example.com\n");
        home.write(
            ".ssh/known_hosts",
            "|1|AAA=|BBB= ssh-ed25519 KEYONE\n|1|CCC=|DDD= ssh-rsa KEYTWO\n",
        );
        let out = read_ssh_hosts(home.path());

        assert_eq!(out.hashed_known_hosts, 2);
        assert!(out.hosts.iter().all(|h| !h.known));
        assert!(out.findings.is_empty());
        // The names are unrecoverable, so no host name is guessed from the file.
        let known = parse_known_hosts("|1|AAA=|BBB= ssh-ed25519 KEYONE\n");
        assert_eq!(known.hashed, 1);
        assert!(!known.is_known("box.example.com"));
    }

    #[test]
    fn plain_known_hosts_entry_marks_that_host_known() {
        let home = TempHome::new("plain-known-hosts");
        home.write(
            ".ssh/config",
            "\
Host box
    HostName box.example.com
Host other
    HostName other.example.com
",
        );
        home.write(
            ".ssh/known_hosts",
            "# a comment\nbox.example.com,10.0.0.1 ssh-ed25519 KEYONE\n[other.example.com]:2222 ssh-rsa KEYTWO\n",
        );
        let out = read_ssh_hosts(home.path());

        assert_eq!(out.hashed_known_hosts, 0);
        assert!(out.hosts.iter().find(|h| h.alias == "box").unwrap().known);
        assert!(out.hosts.iter().find(|h| h.alias == "other").unwrap().known);
        assert!(out.findings.is_empty());
    }

    #[test]
    fn identity_files_are_reported_by_existence_only() {
        let home = TempHome::new("identity-existence");
        home.write(
            ".ssh/config",
            "\
Host present
    HostName p.example.com
    IdentityFile ~/.ssh/deploy_key
Host absent
    HostName a.example.com
    IdentityFile ~/.ssh/keys/gone
    IdentityFile relative_key
",
        );
        home.write(".ssh/deploy_key", "fixture key\n");

        let out = read_ssh_hosts(home.path());
        let by_name = |name: &str| {
            out.key_files
                .iter()
                .find(|k| k.name == name)
                .unwrap_or_else(|| panic!("key file {name} listed"))
                .clone()
        };

        let deploy = by_name("~/.ssh/deploy_key");
        assert_eq!(deploy.path, home.path().join(".ssh/deploy_key"));
        assert!(deploy.exists);

        let gone = by_name("~/.ssh/keys/gone");
        assert_eq!(gone.path, home.path().join(".ssh/keys/gone"));
        assert!(!gone.exists);

        // A bare name is taken under `~/.ssh`.
        let relative = by_name("relative_key");
        assert_eq!(relative.path, home.path().join(".ssh/relative_key"));
        assert!(!relative.exists);
    }

    #[test]
    fn no_returned_value_carries_key_material() {
        let home = TempHome::new("no-key-material");
        home.write(
            ".ssh/config",
            "Host secret\n    HostName secret.example.com\n    IdentityFile ~/.ssh/id_secret\n",
        );
        home.write(
            ".ssh/id_secret",
            "-----BEGIN OPENSSH PRIVATE KEY-----\nSECRETMATERIAL\n-----END OPENSSH PRIVATE KEY-----\n",
        );

        let out = read_ssh_hosts(home.path());
        let rendered = format!(
            "{:?} {:?} {:?} {}",
            out.hosts, out.key_files, out.findings, out.hashed_known_hosts
        );
        assert!(!rendered.contains("SECRETMATERIAL"));
        assert!(!rendered.contains("PRIVATE KEY"));
        for finding in &out.findings {
            assert!(!finding.message().contains("SECRETMATERIAL"));
        }

        // The key shows up as a path with an existence flag, nothing more.
        assert_eq!(out.key_files.len(), 1);
        assert_eq!(out.key_files[0].name, "id_secret");
        assert_eq!(out.key_files[0].path, home.path().join(".ssh/id_secret"));
        assert!(out.key_files[0].exists);
    }
}
