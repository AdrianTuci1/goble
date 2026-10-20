use goble_core::config::{RemoteAgentConfig, RemoteAgentProvider};

use crate::daytona::DaytonaProvider;
use crate::docker::DockerProvider;
use crate::e2b::E2BProvider;
use crate::error::SandboxError;
use crate::modal::ModalProvider;
use crate::provider::SandboxProvider;
use crate::r#box::BoxProvider;
use crate::ssh::SshProvider;

/// Pick the adapter a `[remote_agents.<name>]` table names.
///
/// E2B, Daytona, and Box require an `api_key`; Modal requires a `token_id` plus
/// `token_secret`; Docker needs no credentials; SSH requires the destination in
/// `api_url`. A table whose credential is missing fails here rather than on the
/// first request.
pub fn provider_for(config: &RemoteAgentConfig) -> Result<Box<dyn SandboxProvider>, SandboxError> {
    match config.provider {
        RemoteAgentProvider::E2b => Ok(Box::new(E2BProvider::from_config(config)?)),
        RemoteAgentProvider::Modal => Ok(Box::new(ModalProvider::from_config(config)?)),
        RemoteAgentProvider::Daytona => Ok(Box::new(DaytonaProvider::from_config(config)?)),
        RemoteAgentProvider::Box => Ok(Box::new(BoxProvider::from_config(config)?)),
        RemoteAgentProvider::Docker => Ok(Box::new(DockerProvider::from_config(config))),
        RemoteAgentProvider::Ssh => Ok(Box::new(SshProvider::from_config(config)?)),
    }
}
