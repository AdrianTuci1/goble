use std::collections::BTreeMap;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::SandboxError;

/// The observable result of running a local program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The exit code; `None` when a signal terminated the process.
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    /// Whether the program exited with status zero.
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// What an error message should show: stderr when the program wrote one,
    /// otherwise the exit status.
    pub fn failure_message(&self) -> String {
        let stderr = self.stderr.trim();
        if stderr.is_empty() {
            format!("exit status {:?}", self.status)
        } else {
            stderr.to_string()
        }
    }
}

/// The seam that lets adapters be tested without the real program.
///
/// `env` is added to the inherited environment rather than replacing it, so a
/// provider can pass its token without having to reconstruct `PATH`.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
        env: &BTreeMap<String, String>,
    ) -> Result<CommandOutput, SandboxError>;
}

/// Runs the program on this machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessCommandRunner;

#[async_trait]
impl CommandRunner for ProcessCommandRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
        env: &BTreeMap<String, String>,
    ) -> Result<CommandOutput, SandboxError> {
        let mut child = Command::new(program)
            .args(args)
            .envs(env)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| command_error(program, error))?;

        if let Some(input) = stdin {
            let mut pipe = child.stdin.take().ok_or_else(|| SandboxError::Command {
                command: program.to_string(),
                message: "stdin was not piped".to_string(),
            })?;
            pipe.write_all(input.as_bytes())
                .await
                .map_err(|error| command_error(program, error))?;
            pipe.shutdown()
                .await
                .map_err(|error| command_error(program, error))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|error| command_error(program, error))?;
        Ok(CommandOutput {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn command_error(program: &str, error: impl std::fmt::Display) -> SandboxError {
    SandboxError::Command {
        command: program.to_string(),
        message: error.to_string(),
    }
}
