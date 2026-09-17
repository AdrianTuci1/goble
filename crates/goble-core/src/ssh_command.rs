//! What a submitted command line says about the shell the pane sits in.
//!
//! A line whose own command is `ssh` opens an interactive SSH session, and the
//! session's target is read from that line — the host, an optional user and an
//! optional port — with an `~/.ssh/config` alias resolved through
//! [`crate::ssh_hosts`] to the address it stands for. `exit` is the way back out
//! of such a session. Both reads are pure over the text, so they are tested
//! against fixtures instead of a real shell. Design:
//! `.agents/06-renderer/remote-terminal-renderer.md`.

use crate::ssh_hosts::SshHosts;

/// The port ssh connects to when neither the command line nor the config names
/// one.
pub const DEFAULT_PORT: u16 = 22;

/// Where an interactive `ssh` session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshSession {
    /// The host ssh connects to: the address a `HostName` names, or the name
    /// the command gave when the config declares no block for it.
    pub host: String,
    /// The account ssh authenticates as.
    pub user: String,
    /// The port, [`DEFAULT_PORT`] when nothing names one.
    pub port: u16,
    /// The `~/.ssh/config` alias the command named, when it named one.
    pub alias: Option<String>,
}

impl SshSession {
    /// What a chip naming this session reads: the user and the host as the
    /// session has them, `user@host`. When an `~/.ssh/config` alias was used and
    /// the address it resolves to differs from it, the alias is named with that
    /// address — so both what the user typed and where it lands are on the chip.
    pub fn chip_label(&self) -> String {
        match self.alias.as_deref() {
            Some(alias) if alias != self.host => {
                format!("{}@{} ({})", self.user, alias, self.host)
            }
            _ => format!("{}@{}", self.user, self.host),
        }
    }
}

/// A target as the command line names it, before the config is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    /// The name the command gave: an alias, or the address itself.
    pub host: String,
    /// The user the command named, with `user@host` or `-l`.
    pub user: Option<String>,
    /// The port the command named with `-p`.
    pub port: Option<u16>,
}

impl SshTarget {
    /// Where this target lands with the machine's SSH configuration applied: a
    /// `Host` block supplies the real address (`HostName`), the account and the
    /// port, while what the command line named itself wins over the block. A
    /// name no block declares is connected to as typed, under `local_user`,
    /// which is what ssh does with the account it is running as.
    pub fn resolve(&self, hosts: Option<&SshHosts>, local_user: &str) -> SshSession {
        let block = hosts.and_then(|ssh| {
            ssh.hosts
                .iter()
                .find(|host| host.alias == self.host)
                // ssh matches a `Host` pattern case-insensitively, so an alias
                // typed in another case is the same block.
                .or_else(|| {
                    ssh.hosts
                        .iter()
                        .find(|host| host.alias.eq_ignore_ascii_case(&self.host))
                })
        });
        match block {
            Some(host) => SshSession {
                host: host.hostname.clone(),
                user: self.user.clone().unwrap_or_else(|| host.user.clone()),
                port: self.port.unwrap_or(host.port),
                alias: Some(host.alias.clone()),
            },
            None => SshSession {
                host: self.host.clone(),
                user: self.user.clone().unwrap_or_else(|| local_user.to_string()),
                port: self.port.unwrap_or(DEFAULT_PORT),
                alias: None,
            },
        }
    }
}

/// Read the target of an interactive `ssh` session out of a submitted command
/// line; `None` when the line opens none.
///
/// Only a line whose own command is `ssh` opens one, so a line that merely
/// mentions it — `sudo ssh …`, `grep ssh …`, `ssh-add …`, a `ssh host` inside a
/// quoted argument — is left alone rather than guessed at. Options are walked
/// rather than pattern-matched, so `-p 2222` is the port and not the host; the
/// target is the line's one positional argument, and a second one is a command
/// ssh runs on the remote, which is not a session. The options that make ssh
/// leave nothing to sit in — `-V`, `-Q`, `-G` (ssh prints and exits) and `-T`,
/// `-W` (no terminal, or a forwarded stream) — are not sessions either.
pub fn parse_ssh_command(command: &str) -> Option<SshTarget> {
    let tokens = split_words(command)?;
    if tokens.first().map(String::as_str) != Some("ssh") {
        return None;
    }
    let mut host: Option<String> = None;
    let mut user: Option<String> = None;
    let mut port: Option<u16> = None;

    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        match token {
            // ssh prints what was asked and exits, and a session with no tty or
            // a forwarded stdio is not one the user sits in.
            "-V" | "-Q" | "-G" | "-T" | "-W" => return None,
            "-p" => {
                i += 1;
                port = Some(port_of(tokens.get(i)?)?);
            }
            "-l" => {
                i += 1;
                user = Some(tokens.get(i)?.clone());
            }
            _ if token.starts_with("-p") && token.len() > 2 => {
                port = Some(port_of(&token[2..])?);
            }
            _ if token.starts_with("-l") && token.len() > 2 => {
                user = Some(token[2..].to_string());
            }
            // An option that takes a value: the value is not the target.
            _ if takes_value(token) => {
                i += 1;
                tokens.get(i)?;
            }
            // Any other option, and `--`, change nothing about the target.
            _ if token.starts_with('-') && token.len() > 1 => {}
            _ => {
                if host.is_some() {
                    // A second positional is a command for the remote to run:
                    // ssh exits with it instead of leaving the session open.
                    return None;
                }
                // `user@host` and `-l user` both name the account; the later one
                // of the two is the one the command line ends up with.
                match token.split_once('@') {
                    Some((named, rest)) if !named.is_empty() => {
                        if rest.is_empty() {
                            // `user@` names no host to connect to.
                            return None;
                        }
                        user = Some(named.to_string());
                        host = Some(rest.to_string());
                    }
                    _ => host = Some(token.to_string()),
                }
            }
        }
        i += 1;
    }

    Some(SshTarget {
        host: host?,
        user,
        port,
    })
}

/// Whether the line is the way out of the shell the pane is in: `exit`, or
/// `logout`, typed at that shell. The command must be the line's own — `echo
/// exit` is not a way out of anything.
pub fn returns_to_local_shell(command: &str) -> bool {
    let word = split_words(command).and_then(|tokens| tokens.into_iter().next());
    matches!(word.as_deref(), Some("exit") | Some("logout"))
}

/// The options that take a value, so their value is skipped rather than read as
/// the target. ssh's own set, short of the ones handled above.
fn takes_value(token: &str) -> bool {
    matches!(
        token,
        "-B" | "-b"
            | "-c"
            | "-D"
            | "-E"
            | "-e"
            | "-F"
            | "-I"
            | "-i"
            | "-J"
            | "-L"
            | "-m"
            | "-O"
            | "-o"
            | "-P"
            | "-R"
            | "-S"
            | "-w"
    )
}

/// A port as `-p` names it. A port ssh would refuse is not a session to bind.
fn port_of(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().filter(|port| *port != 0)
}

/// Split a command line into words: whitespace separates, `'…'` is literal,
/// `"…"` and a backslash escape quote. `None` when a quote is left open, so a
/// line that is not a command is never read as one.
fn split_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut started = false;

    for c in command.chars() {
        if escaped {
            word.push(c);
            escaped = false;
            continue;
        }
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else {
                    word.push(c);
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                    started = true;
                } else if c == '\\' {
                    escaped = true;
                    started = true;
                } else if c.is_whitespace() {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                } else {
                    word.push(c);
                    started = true;
                }
            }
        }
    }
    // The line is read as it was submitted, not as a shell would join it, so a
    // trailing backslash stays a backslash.
    if escaped {
        word.push('\\');
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
Host web
    HostName web.example.com
    User deploy
    Port 2222
Host db
    HostName db.example.com
";

    fn target(command: &str) -> SshTarget {
        parse_ssh_command(command).unwrap_or_else(|| panic!("{command} opens a session"))
    }

    /// The config above, as `read_ssh_hosts` would hand it back — parsed, not
    /// read from a directory, so no test touches a real `~/.ssh`.
    fn hosts() -> SshHosts {
        let ssh_dir = std::path::PathBuf::from("/nonexistent/.ssh");
        SshHosts {
            config_path: ssh_dir.join("config"),
            hosts: crate::ssh_hosts::parse_ssh_config(CONFIG, "fallback"),
            key_files: Vec::new(),
            findings: Vec::new(),
            hashed_known_hosts: 0,
            ssh_dir,
        }
    }

    fn session(command: &str) -> SshSession {
        target(command).resolve(Some(&hosts()), "me")
    }

    #[test]
    fn a_bare_host_is_the_target() {
        assert_eq!(
            target("ssh web"),
            SshTarget {
                host: "web".to_string(),
                user: None,
                port: None,
            }
        );
        // Leading whitespace is the shell's, not the command's.
        assert_eq!(target("  ssh web ").host, "web");
    }

    #[test]
    fn user_at_host_names_the_account_and_the_host() {
        let parsed = target("ssh ada@server.example.com");
        assert_eq!(parsed.host, "server.example.com");
        assert_eq!(parsed.user.as_deref(), Some("ada"));
        assert_eq!(parsed.port, None);
    }

    #[test]
    fn the_port_comes_from_dash_p_either_way_it_is_written() {
        assert_eq!(target("ssh -p 2222 host").port, Some(2222));
        assert_eq!(target("ssh -p2222 host").port, Some(2222));
        assert_eq!(target("ssh host").port, None);
    }

    #[test]
    fn dash_l_names_the_account() {
        assert_eq!(target("ssh -l root host").user.as_deref(), Some("root"));
        assert_eq!(target("ssh -lroot host").user.as_deref(), Some("root"));
    }

    #[test]
    fn an_option_value_is_not_the_target() {
        assert_eq!(target("ssh -i /keys/id_x host").host, "host");
        assert_eq!(target("ssh -o StrictHostKeyChecking=no host").host, "host");
        assert_eq!(target("ssh -F /tmp/ssh_config host").host, "host");
        // The login name and the host may both be named; the later one wins.
        assert_eq!(target("ssh -l root ada@host").user.as_deref(), Some("ada"));
        assert_eq!(target("ssh ada@host -l root").user.as_deref(), Some("root"));
    }

    #[test]
    fn a_quoted_or_escaped_host_is_one_word() {
        assert_eq!(target("ssh \"web\"").host, "web");
        assert_eq!(target("ssh 'web'").host, "web");
        // An open quote is not a command line.
        assert_eq!(parse_ssh_command("ssh \"web"), None);
    }

    #[test]
    fn a_line_that_merely_mentions_ssh_is_not_a_session() {
        assert_eq!(parse_ssh_command("sudo ssh web"), None);
        assert_eq!(parse_ssh_command("grep ssh /etc/hosts"), None);
        assert_eq!(parse_ssh_command("echo \"ssh web\""), None);
        assert_eq!(parse_ssh_command("scp -p 2222 file web:/tmp"), None);
        assert_eq!(parse_ssh_command("sftp web"), None);
        assert_eq!(parse_ssh_command("ssh-add -l"), None);
        assert_eq!(parse_ssh_command("sshd -t"), None);
        assert_eq!(parse_ssh_command("ls"), None);
        assert_eq!(parse_ssh_command(""), None);
    }

    #[test]
    fn an_ssh_that_prints_or_forwards_is_not_a_session() {
        assert_eq!(parse_ssh_command("ssh -V"), None);
        assert_eq!(parse_ssh_command("ssh -Q cipher"), None);
        assert_eq!(parse_ssh_command("ssh -G host"), None);
        assert_eq!(parse_ssh_command("ssh -T git@github.com"), None);
        assert_eq!(parse_ssh_command("ssh -W host:22 jump"), None);
        // A bare `ssh` names no host at all.
        assert_eq!(parse_ssh_command("ssh"), None);
        assert_eq!(parse_ssh_command("ssh -v"), None);
    }

    #[test]
    fn a_remote_command_is_not_an_interactive_session() {
        assert_eq!(parse_ssh_command("ssh host true"), None);
        assert_eq!(parse_ssh_command("ssh -o BatchMode=yes host uptime"), None);
        assert_eq!(parse_ssh_command("ssh user@host ls -la"), None);
    }

    #[test]
    fn a_port_ssh_would_refuse_is_not_a_session() {
        assert_eq!(parse_ssh_command("ssh -p abc host"), None);
        assert_eq!(parse_ssh_command("ssh -p 0 host"), None);
        assert_eq!(parse_ssh_command("ssh -p 70000 host"), None);
        assert_eq!(parse_ssh_command("ssh -p"), None);
        assert_eq!(parse_ssh_command("ssh user@"), None);
    }

    #[test]
    fn an_alias_resolves_through_the_config() {
        let web = session("ssh web");
        assert_eq!(web.host, "web.example.com");
        assert_eq!(web.user, "deploy");
        assert_eq!(web.port, 2222);
        assert_eq!(web.alias.as_deref(), Some("web"));

        // A block with no User and no Port keeps ssh's own defaults: the
        // account the config was read with, and the standard port.
        let db = session("ssh db");
        assert_eq!(db.host, "db.example.com");
        assert_eq!(db.user, "fallback");
        assert_eq!(db.port, DEFAULT_PORT);
        assert_eq!(db.alias.as_deref(), Some("db"));
    }

    #[test]
    fn the_chip_names_the_user_and_host_as_the_session_has_them() {
        // A name the config knows nothing about is the host itself, under the
        // account the command named.
        assert_eq!(
            session("ssh ada@server.example.com").chip_label(),
            "ada@server.example.com"
        );
        assert_eq!(session("ssh -l root host").chip_label(), "root@host");

        // An alias names what was typed and the address it stands for.
        assert_eq!(
            session("ssh web").chip_label(),
            "deploy@web (web.example.com)"
        );
        assert_eq!(
            session("ssh ada@web").chip_label(),
            "ada@web (web.example.com)"
        );

        // A block that keeps the name it was declared under names it once.
        let same = SshSession {
            host: "web".to_string(),
            user: "deploy".to_string(),
            port: DEFAULT_PORT,
            alias: Some("web".to_string()),
        };
        assert_eq!(same.chip_label(), "deploy@web");
    }

    #[test]
    fn what_the_command_names_wins_over_the_config() {
        let session = session("ssh -p 2200 ada@web");
        assert_eq!(session.host, "web.example.com");
        assert_eq!(session.user, "ada");
        assert_eq!(session.port, 2200);
        assert_eq!(session.alias.as_deref(), Some("web"));
    }

    #[test]
    fn an_alias_is_matched_in_any_case_and_an_unknown_name_is_its_own_host() {
        let web = session("ssh WEB");
        assert_eq!(web.host, "web.example.com");
        assert_eq!(web.alias.as_deref(), Some("web"));

        let unknown = session("ssh other.example.com");
        assert_eq!(unknown.host, "other.example.com");
        assert_eq!(unknown.user, "me");
        assert_eq!(unknown.port, DEFAULT_PORT);
        assert_eq!(unknown.alias, None);
    }

    #[test]
    fn a_machine_with_no_config_still_resolves_the_name_it_was_given() {
        let unknown = target("ssh -p 2222 server").resolve(None, "me");
        assert_eq!(unknown.host, "server");
        assert_eq!(unknown.user, "me");
        assert_eq!(unknown.port, 2222);
        assert_eq!(unknown.alias, None);
    }

    #[test]
    fn exit_and_logout_are_the_way_back_to_a_local_shell() {
        assert!(returns_to_local_shell("exit"));
        assert!(returns_to_local_shell("  exit "));
        assert!(returns_to_local_shell("exit 1"));
        assert!(returns_to_local_shell("logout"));
        assert!(!returns_to_local_shell("ls"));
        assert!(!returns_to_local_shell("echo exit"));
        assert!(!returns_to_local_shell("ssh exit"));
        assert!(!returns_to_local_shell(""));
    }
}
