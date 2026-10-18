//! Saved SSH hosts: the user's own list of machines a pane can open.
//!
//! The file is `~/.goble/hosts.toml`, beside `config.toml`, and every entry is
//! one `[[hosts]]` table:
//!
//! ```toml
//! [[hosts]]
//! name = "prod"
//! host = "prod.example.com"
//! user = "deploy"
//! port = 2222
//! identity = "~/.ssh/id_ed25519"
//! ```
//!
//! `host` is the only required address field; `user`, `port` and `identity` are
//! optional and fall back to ssh's own defaults. The parse and the argv are
//! pure, so they are tested against fixtures rather than a live host.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ssh_command::{SshSession, DEFAULT_PORT};
use crate::ssh_hosts::current_user;

/// The file this module reads and writes under `~/.goble`.
pub const HOSTS_FILE_NAME: &str = "hosts.toml";

/// The commented header a fresh `hosts.toml` carries, so the file documents its
/// own shape the way `environment.toml` does.
pub const HOSTS_HEADER: &str = "\
# Saved SSH hosts opened from a pane (Cmd+Shift+N).
#
# [[hosts]]
# name = \"prod\"
# host = \"prod.example.com\"
# user = \"deploy\"        # optional; ssh's default when omitted
# port = 2222             # optional; 22 when omitted
# identity = \"~/.ssh/id_ed25519\"  # optional
";

/// One saved host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostEntry {
    /// The name the host picker draws. It is a label, not an ssh alias.
    pub name: String,
    /// The address ssh connects to.
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    /// An `IdentityFile` to pass with `-i`. `~/` is expanded when the command
    /// line is built.
    #[serde(default)]
    pub identity: Option<String>,
}

impl HostEntry {
    /// Whether the entry names enough to connect: a name to draw and a host to
    /// reach. A row missing either is skipped rather than offered.
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && !self.host.trim().is_empty()
    }

    /// The account ssh will authenticate as.
    pub fn user_or_local(&self) -> String {
        self.user
            .clone()
            .filter(|user| !user.trim().is_empty())
            .unwrap_or_else(current_user)
    }

    /// The port ssh will connect to.
    pub fn port_or_default(&self) -> u16 {
        self.port.unwrap_or(DEFAULT_PORT)
    }

    /// The target the picker shows: `user@host:port`.
    pub fn target(&self) -> String {
        format!("{}@{}:{}", self.user_or_local(), self.host, self.port_or_default())
    }

    /// The command line a pane runs for this host, `ssh -t` with the entry's
    /// port, identity and account. Arguments are quoted for a POSIX shell, and
    /// an identity written with `~/` is expanded first so quoting does not stop
    /// the shell from finding it.
    pub fn command_line(&self) -> String {
        let home = dirs::home_dir();
        self.command_line_with_home(home.as_deref())
    }

    /// [`Self::command_line`] against an explicit home, for tests and for a
    /// caller that already resolved the Goble home.
    pub fn command_line_with_home(&self, home: Option<&Path>) -> String {
        self.ssh_argv_with_home(home)
            .iter()
            .map(|arg| quote_arg(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The argv the pane hands to the shell: the system `ssh`, `-t` so an
    /// interactive session gets a tty, then the entry's options and target.
    pub fn ssh_argv(&self) -> Vec<String> {
        self.ssh_argv_with_home(None)
    }

    /// [`Self::ssh_argv`] with `~/` expanded against `home`.
    pub fn ssh_argv_with_home(&self, home: Option<&Path>) -> Vec<String> {
        let mut argv = vec!["ssh".to_string(), "-t".to_string()];
        if self.port.is_some() {
            argv.push("-p".to_string());
            argv.push(self.port_or_default().to_string());
        }
        if let Some(identity) = self.identity.as_deref().filter(|value| !value.trim().is_empty()) {
            argv.push("-i".to_string());
            argv.push(expand_tilde(identity, home));
        }
        if let Some(user) = self.user.as_deref().filter(|value| !value.trim().is_empty()) {
            argv.push("-l".to_string());
            argv.push(user.to_string());
        }
        argv.push(self.host.clone());
        argv
    }

    /// Where this host lands as a session binding: the target the existing SSH
    /// chip and command routing read.
    pub fn ssh_session(&self) -> SshSession {
        SshSession {
            host: self.host.clone(),
            user: self.user_or_local(),
            port: self.port_or_default(),
            alias: None,
        }
    }
}

/// The file's top level: one table per saved host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostsFile {
    #[serde(default)]
    pub hosts: Vec<HostEntry>,
}

impl HostsFile {
    /// Parse `hosts.toml` from text. A missing optional field takes its ssh
    /// default; a row without `name` or `host` fails to parse, and an empty
    /// name or address is skipped by [`Self::valid_hosts`].
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).context("parse hosts.toml")
    }

    /// Read the hosts file at `path`. A missing file is an empty list, not an
    /// error: a user who has never saved a host has nothing to pick from.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
        }
    }

    /// The entries a picker may offer: complete rows only, in file order.
    pub fn valid_hosts(&self) -> Vec<HostEntry> {
        self.hosts.iter().filter(|host| host.is_valid()).cloned().collect()
    }

    /// The path this file lives at under a Goble home.
    pub fn path(home: &Path) -> PathBuf {
        home.join(HOSTS_FILE_NAME)
    }
}

/// The seed text `GobleHome::ensure_base` writes when no hosts file exists.
pub fn empty_toml() -> String {
    format!("{HOSTS_HEADER}\n")
}

/// Expand a leading `~/` against `home`. A path without the prefix is returned
/// unchanged, and a `~` with no home resolved stays literal rather than
/// becoming a relative path.
fn expand_tilde(value: &str, home: Option<&Path>) -> String {
    let Some(rest) = value.strip_prefix("~/") else {
        return value.to_string();
    };
    match home {
        Some(home) => home.join(rest).to_string_lossy().to_string(),
        None => value.to_string(),
    }
}

/// Quote one argument for a POSIX shell. The usual safe set is left bare so the
/// command reads as the user would type it; everything else is single-quoted.
fn quote_arg(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '@' | '%' | '_' | '-' | '.' | '/' | ':' | '=')
            })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
[[hosts]]
name = \"prod\"
host = \"prod.example.com\"
user = \"deploy\"
port = 2222
identity = \"~/.ssh/id_prod\"

[[hosts]]
name = \"db\"
host = \"db.internal\"
";

    #[test]
    fn a_hosts_file_parses_every_field_and_defaults_the_optional_ones() {
        let file = HostsFile::from_toml(FILE).expect("valid hosts.toml");
        assert_eq!(file.hosts.len(), 2);
        let prod = &file.hosts[0];
        assert_eq!(prod.name, "prod");
        assert_eq!(prod.host, "prod.example.com");
        assert_eq!(prod.user.as_deref(), Some("deploy"));
        assert_eq!(prod.port, Some(2222));
        assert_eq!(prod.identity.as_deref(), Some("~/.ssh/id_prod"));

        let db = &file.hosts[1];
        assert_eq!(db.name, "db");
        assert_eq!(db.host, "db.internal");
        assert_eq!(db.user, None);
        assert_eq!(db.port, None);
        assert_eq!(db.identity, None);
        assert_eq!(db.port_or_default(), DEFAULT_PORT);
    }

    #[test]
    fn a_host_without_a_name_or_address_is_not_offered() {
        let file = HostsFile::from_toml(
            "[[hosts]]\nname = \"\"\nhost = \"somewhere\"\n\
             [[hosts]]\nname = \"named\"\nhost = \"\"\n\
             [[hosts]]\nname = \"ok\"\nhost = \"ok.example.com\"\n",
        )
        .expect("valid toml");
        let hosts = file.valid_hosts();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "ok");
    }

    #[test]
    fn a_malformed_file_is_an_error_and_a_missing_file_is_empty() {
        assert!(HostsFile::from_toml("[[hosts]]\nname = \"x\"\n").is_err());

        let dir = tempfile::tempdir().expect("temp dir");
        let missing = HostsFile::load(&dir.path().join(HOSTS_FILE_NAME)).expect("missing is empty");
        assert!(missing.hosts.is_empty());
    }

    #[test]
    fn the_argv_is_ssh_dash_t_with_the_entrys_options() {
        let file = HostsFile::from_toml(FILE).expect("valid hosts.toml");
        assert_eq!(
            file.hosts[0].ssh_argv(),
            vec![
                "ssh",
                "-t",
                "-p",
                "2222",
                "-i",
                "~/.ssh/id_prod",
                "-l",
                "deploy",
                "prod.example.com",
            ]
        );
        // A minimal entry asks for nothing but a tty on the address.
        assert_eq!(file.hosts[1].ssh_argv(), vec!["ssh", "-t", "db.internal"]);
    }

    #[test]
    fn the_identity_is_expanded_before_it_is_quoted() {
        let file = HostsFile::from_toml(FILE).expect("valid hosts.toml");
        let home = Path::new("/home/ada");
        assert_eq!(
            file.hosts[0].command_line_with_home(Some(home)),
            "ssh -t -p 2222 -i /home/ada/.ssh/id_prod -l deploy prod.example.com"
        );
        // The generated line is the same shape the interactive parser binds.
        let target = crate::ssh_command::parse_ssh_command(
            &file.hosts[0].command_line_with_home(Some(home)),
        )
        .expect("the generated line opens a session");
        assert_eq!(target.host, "prod.example.com");
        assert_eq!(target.user.as_deref(), Some("deploy"));
        assert_eq!(target.port, Some(2222));
    }

    #[test]
    fn an_argument_the_shell_would_change_is_quoted() {
        let host = HostEntry {
            name: "odd".to_string(),
            host: "host with space".to_string(),
            user: None,
            port: None,
            identity: None,
        };
        assert!(host.command_line_with_home(None).ends_with("'host with space'"));
    }

    #[test]
    fn a_saved_host_resolves_to_the_session_the_chip_names() {
        let file = HostsFile::from_toml(FILE).expect("valid hosts.toml");
        let session = file.hosts[0].ssh_session();
        assert_eq!(session.host, "prod.example.com");
        assert_eq!(session.user, "deploy");
        assert_eq!(session.port, 2222);
        assert_eq!(session.chip_label(), "deploy@prod.example.com");
    }
}
