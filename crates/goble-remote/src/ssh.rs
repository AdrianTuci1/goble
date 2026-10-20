use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::command::{CommandOutput, CommandRunner, ProcessCommandRunner};
use crate::error::SandboxError;
use crate::provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};

const PROVIDER: &str = "ssh";
const SSH_PROGRAM: &str = "ssh";
const SCP_PROGRAM: &str = "scp";
const CONNECT_TIMEOUT_SECS: &str = "10";
const DEFAULT_DIR_PREFIX: &str = "/tmp/goble-";
const WORKER_BINARY: &str = "goblin";

/// A worker on an existing host, driven through the local `ssh` and `scp`.
///
/// The config's `api_url` is the destination (`user@host` or
/// `ssh://user@host:port`), and `image` names either an absolute remote install
/// directory (already holding `goblin`), an `http(s)` URL for the worker binary,
/// or a local file that is copied with `scp`. Each operation is one remote shell
/// script, executed as a single `ssh` argument, so tests can assert the exact
/// argv without touching the network.
///
/// The worker is started with `nohup env KEY=value ... goblin`, and its pid is
/// kept in `<install_dir>/goble-worker.pid` for status and teardown.
pub struct SshProvider {
    target: SshTarget,
    image: Option<String>,
    runner: Arc<dyn CommandRunner>,
}

impl SshProvider {
    /// Build an adapter around an already-resolved runner.
    pub fn with_runner(
        target: impl AsRef<str>,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Self, SandboxError> {
        Ok(Self {
            target: SshTarget::parse(target.as_ref())?,
            image: None,
            runner,
        })
    }

    /// Build an adapter that runs the real `ssh` and `scp` executables.
    pub fn new(target: impl AsRef<str>) -> Result<Self, SandboxError> {
        Self::with_runner(target, Arc::new(ProcessCommandRunner))
    }

    /// Build from a `[remote_agents.<name>]` table.
    pub fn from_config(
        config: &goble_core::config::RemoteAgentConfig,
    ) -> Result<Self, SandboxError> {
        let target = config
            .api_url
            .as_deref()
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .ok_or_else(|| SandboxError::InvalidRequest {
                message: "ssh needs a target in api_url (user@host or ssh://user@host:port)"
                    .to_string(),
            })?;
        Ok(Self::with_runner(target, Arc::new(ProcessCommandRunner))?
            .with_image(config.image.clone()))
    }

    /// Set the fallback install directory or worker binary URL.
    pub fn with_image(mut self, image: Option<String>) -> Self {
        self.image = image
            .as_deref()
            .map(str::trim)
            .filter(|image| !image.is_empty())
            .map(str::to_string);
        self
    }

    fn effective_image<'a>(&'a self, request: &'a SandboxRequest) -> Option<&'a str> {
        let image = request.image.trim();
        if !image.is_empty() {
            Some(image)
        } else {
            self.image.as_deref()
        }
    }

    async fn ssh(&self, script: String) -> Result<CommandOutput, SandboxError> {
        let mut args = self.target.ssh_args();
        args.push(script);
        self.runner
            .run(SSH_PROGRAM, &args, None, &BTreeMap::new())
            .await
    }

    async fn checked(
        &self,
        output: CommandOutput,
        command: impl Into<String>,
    ) -> Result<CommandOutput, SandboxError> {
        if output.success() {
            Ok(output)
        } else {
            Err(SandboxError::Command {
                command: command.into(),
                message: output.failure_message(),
            })
        }
    }

    fn install_dir_for(&self, handle: &SandboxHandle) -> String {
        let uri = self.target.uri();
        if let Some(dir) = handle.sandbox_id.strip_prefix(&uri) {
            if dir.starts_with('/') {
                return dir.to_string();
            }
        }
        handle.sandbox_id.clone()
    }

    fn destroy_script(&self, dir: &str) -> String {
        format!(
            "set -e\npid_file={pid}\nif [ -f \"$pid_file\" ]; then\n  pid=$(cat \"$pid_file\")\n  if [ -n \"$pid\" ]; then\n    kill \"$pid\" 2>/dev/null || true\n    n=0\n    while kill -0 \"$pid\" 2>/dev/null && [ \"$n\" -lt 50 ]; do\n      sleep 0.1\n      n=$((n + 1))\n    done\n    kill -9 \"$pid\" 2>/dev/null || true\n  fi\nfi\nif pgrep -f -- \"$dir/goblin\" >/dev/null 2>&1; then\n  pkill -f -- \"$dir/goblin\" 2>/dev/null || true\n  sleep 0.2\n  pkill -9 -f -- \"$dir/goblin\" 2>/dev/null || true\nfi\nrm -rf {dir}",
            pid = shell_quote(&format!("{dir}/goble-worker.pid")),
            dir = shell_quote(dir),
        )
    }

    fn status_script(&self, dir: &str) -> String {
        format!(
            "dir={dir}\npid_file=\"$dir/goble-worker.pid\"\nif [ ! -d \"$dir\" ]; then\n  echo missing\n  exit 0\nfi\nif [ -f \"$pid_file\" ]; then\n  pid=$(cat \"$pid_file\")\n  if [ -n \"$pid\" ] && kill -0 \"$pid\" 2>/dev/null; then\n    echo running\n    exit 0\n  fi\nfi\nif pgrep -f -- \"$dir/goblin\" >/dev/null 2>&1; then\n  echo running\nelse\n  echo paused\nfi",
            dir = shell_quote(dir),
        )
    }
}

#[async_trait]
impl SandboxProvider for SshProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        let image = self
            .effective_image(request)
            .ok_or_else(|| SandboxError::InvalidRequest {
                message: "ssh needs a remote install directory or a worker binary URL in image"
                    .to_string(),
            })?;
        let default_dir = format!("{DEFAULT_DIR_PREFIX}{}", request.sandbox_name);
        let (dir, install) = if image.starts_with("http://") || image.starts_with("https://") {
            (default_dir, Install::Download(image.to_string()))
        } else if Path::new(image).is_file() {
            (default_dir, Install::LocalFile(image.to_string()))
        } else if image.starts_with('/') {
            (image.to_string(), Install::RemoteDirectory)
        } else {
            return Err(SandboxError::InvalidRequest {
                message: format!(
                    "ssh image {image:?} must be an absolute remote directory, a local file, or an http(s) URL"
                ),
            });
        };

        let mkdir = format!("mkdir -p {}", shell_quote(&dir));
        let output = self.ssh(mkdir).await?;
        self.checked(output, format!("ssh mkdir -p {dir}")).await?;

        match &install {
            Install::Download(url) => {
                let script = format!(
                    "set -e\ncurl -fsSL {url} -o {binary}\nchmod +x {binary}",
                    url = shell_quote(url),
                    binary = shell_quote(&format!("{dir}/{WORKER_BINARY}")),
                );
                let output = self.ssh(script).await?;
                self.checked(output, format!("ssh curl {url}")).await?;
            }
            Install::LocalFile(local) => {
                let remote = format!("{dir}/{WORKER_BINARY}");
                let output = self
                    .runner
                    .run(
                        SCP_PROGRAM,
                        &self.target.scp_args(local, &remote),
                        None,
                        &BTreeMap::new(),
                    )
                    .await?;
                self.checked(output, format!("scp {local} {remote}"))
                    .await?;
            }
            Install::RemoteDirectory => {}
        }

        let start = start_script(request, &dir);
        let output = self.ssh(start).await?;
        self.checked(output, format!("ssh start {}", request.sandbox_name))
            .await?;

        Ok(SandboxHandle::from_request(
            PROVIDER,
            format!("{}{}", self.target.uri(), dir),
            request,
        ))
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let dir = self.install_dir_for(handle);
        let output = self.ssh(self.destroy_script(&dir)).await?;
        self.checked(output, format!("ssh rm -rf {dir}")).await?;
        Ok(())
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
        let dir = self.install_dir_for(handle);
        let output = self.ssh(self.status_script(&dir)).await?;
        let output = self.checked(output, format!("ssh status {dir}")).await?;
        Ok(match output.stdout.trim() {
            "running" => SandboxStatus::Running,
            "paused" => SandboxStatus::Paused,
            "missing" => SandboxStatus::Missing,
            _ => SandboxStatus::Unknown,
        })
    }
}

enum Install {
    Download(String),
    LocalFile(String),
    RemoteDirectory,
}

fn start_script(request: &SandboxRequest, dir: &str) -> String {
    let mut command = String::from("nohup env");
    for (key, value) in &request.env {
        command.push(' ');
        command.push_str(&shell_quote(&format!("{key}={value}")));
    }
    if request.command.is_empty() {
        command.push(' ');
        command.push_str(&shell_quote(&format!("{dir}/{WORKER_BINARY}")));
    } else {
        for arg in &request.command {
            command.push(' ');
            command.push_str(&shell_quote(arg));
        }
    }

    let script = format!("set -e\ncd {}", shell_quote(dir));
    if request.command.is_empty() {
        format!(
            "{script}\ntest -x {binary}\n{command} >> {log} 2>&1 &\necho $! > {pid}\necho started",
            binary = shell_quote(&format!("{dir}/{WORKER_BINARY}")),
            log = shell_quote(&format!("{dir}/goble-worker.log")),
            pid = shell_quote(&format!("{dir}/goble-worker.pid")),
        )
    } else {
        format!(
            "{script}\n{command} >> {log} 2>&1 &\necho $! > {pid}\necho started",
            log = shell_quote(&format!("{dir}/goble-worker.log")),
            pid = shell_quote(&format!("{dir}/goble-worker.pid")),
        )
    }
}

/// Quote one shell word, including the empty string.
fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SshTarget {
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

impl SshTarget {
    fn parse(value: &str) -> Result<Self, SandboxError> {
        let raw = value.trim();
        if raw.is_empty() {
            return Err(SandboxError::InvalidRequest {
                message: "ssh target is empty".to_string(),
            });
        }
        let rest = raw.strip_prefix("ssh://").unwrap_or(raw);
        if rest.is_empty() || rest.chars().any(char::is_whitespace) {
            return Err(SandboxError::InvalidRequest {
                message: format!("ssh target {value:?} is not user@host or ssh://user@host:port"),
            });
        }
        let (user, host_port) = match rest.rsplit_once('@') {
            Some((user, host_port)) => {
                if user.is_empty() || host_port.is_empty() {
                    return Err(SandboxError::InvalidRequest {
                        message: format!("ssh target {value:?} has an empty user or host"),
                    });
                }
                (Some(user.to_string()), host_port)
            }
            None => (None, rest),
        };
        if host_port.contains('/') {
            return Err(SandboxError::InvalidRequest {
                message: format!("ssh target {value:?} must not carry a path"),
            });
        }
        let (host, port) = split_host_port(host_port, value)?;
        Ok(Self { user, host, port })
    }

    fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    fn uri(&self) -> String {
        let mut uri = format!("ssh://{}", self.destination());
        if let Some(port) = self.port {
            uri.push(':');
            uri.push_str(&port.to_string());
        }
        uri
    }

    fn ssh_args(&self) -> Vec<String> {
        let mut args = connection_options();
        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        args.push(self.destination());
        args
    }

    fn scp_args(&self, local: &str, remote: &str) -> Vec<String> {
        let mut args = connection_options();
        if let Some(port) = self.port {
            args.push("-P".to_string());
            args.push(port.to_string());
        }
        args.push(local.to_string());
        args.push(format!("{}:{remote}", self.destination()));
        args
    }
}

fn connection_options() -> Vec<String> {
    vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"),
    ]
}

fn split_host_port(host_port: &str, original: &str) -> Result<(String, Option<u16>), SandboxError> {
    let invalid = || SandboxError::InvalidRequest {
        message: format!("ssh target {original:?} has an invalid host or port"),
    };

    if let Some(rest) = host_port.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(invalid)?;
        let host = format!("[{}]", &rest[..end]);
        let suffix = &rest[end + 1..];
        if suffix.is_empty() {
            return Ok((host, None));
        }
        let port = suffix.strip_prefix(':').ok_or_else(invalid)?;
        return Ok((host, Some(parse_port(port, original)?)));
    }

    match host_port.rsplit_once(':') {
        Some((host, port)) => {
            if host.is_empty() {
                return Err(invalid());
            }
            Ok((host.to_string(), Some(parse_port(port, original)?)))
        }
        None => Ok((host_port.to_string(), None)),
    }
}

fn parse_port(value: &str, original: &str) -> Result<u16, SandboxError> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| SandboxError::InvalidRequest {
            message: format!("ssh target {original:?} has an invalid port {value:?}"),
        })
}
