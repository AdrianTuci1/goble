use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::command::{CommandOutput, CommandRunner, ProcessCommandRunner};
use crate::error::SandboxError;
use crate::provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};

const PROVIDER: &str = "docker";
const PROGRAM: &str = "docker";

/// Docker through the local CLI.
///
/// Containers are named with the stable sandbox name and labelled so `docker
/// ps --filter label=goble.sandbox=1` finds them. There is no inbound port and
/// no VNC: the worker inside connects out to the gateway.
pub struct DockerProvider {
    runner: Arc<dyn CommandRunner>,
}

impl DockerProvider {
    /// Build an adapter that runs the real `docker` executable.
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessCommandRunner),
        }
    }

    /// Build around an already-resolved runner.
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }

    /// Docker needs no credentials, so this only exists for factory symmetry.
    pub fn from_config(_config: &goble_core::config::RemoteAgentConfig) -> Self {
        Self::new()
    }

    async fn run(&self, args: Vec<String>) -> Result<CommandOutput, SandboxError> {
        let env = BTreeMap::new();
        self.runner.run(PROGRAM, &args, None, &env).await
    }
}

impl Default for DockerProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxProvider for DockerProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            request.sandbox_name.clone(),
            "--label".to_string(),
            "goble.sandbox=1".to_string(),
            "--label".to_string(),
            format!("goble.project={}", request.project_id),
        ];
        for (key, value) in &request.env {
            args.push("-e".to_string());
            args.push(format!("{key}={value}"));
        }
        args.push(request.image.clone());
        args.extend(request.command.iter().cloned());

        let output = self.run(args).await?;
        if !output.success() {
            return Err(SandboxError::Command {
                command: format!("docker run {}", request.sandbox_name),
                message: output.failure_message(),
            });
        }
        let sandbox_id = output.stdout.trim();
        if sandbox_id.is_empty() {
            return Err(SandboxError::Parse {
                context: "docker run output",
                message: "docker printed no container id".to_string(),
            });
        }
        Ok(SandboxHandle::from_request(PROVIDER, sandbox_id, request))
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let output = self
            .run(vec![
                "rm".to_string(),
                "-f".to_string(),
                handle.sandbox_name.clone(),
            ])
            .await?;
        if !output.success() {
            return Err(SandboxError::Command {
                command: format!("docker rm -f {}", handle.sandbox_name),
                message: output.failure_message(),
            });
        }
        Ok(())
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
        let output = self
            .run(vec![
                "inspect".to_string(),
                "-f".to_string(),
                "{{.State.Running}}".to_string(),
                handle.sandbox_name.clone(),
            ])
            .await?;
        if !output.success() {
            // A missing container, or Docker refusing to inspect it, means the
            // provider has nothing for this handle.
            return Ok(SandboxStatus::Missing);
        }
        Ok(match output.stdout.trim() {
            "true" => SandboxStatus::Running,
            // Docker has no paused state exposed by `.State.Running`; a
            // stopped container is the closest thing to a paused sandbox.
            "false" => SandboxStatus::Paused,
            _ => SandboxStatus::Unknown,
        })
    }
}
