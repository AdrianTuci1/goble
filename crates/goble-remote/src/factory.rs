use goble_core::config::{RemoteAgentConfig, RemoteAgentProvider};

use crate::docker::DockerProvider;
use crate::e2b::E2BProvider;
use crate::error::SandboxError;
use crate::modal::ModalProvider;
use crate::provider::SandboxProvider;

/// Pick the adapter a `[remote_agents.<name>]` table names.
///
/// Daytona, Box, and SSH are declared in the config schema but have no adapter
/// yet; they fail with [`SandboxError::Unsupported`] instead of being guessed
/// at. Adding one is a trait implementation plus a match arm here.
pub fn provider_for(config: &RemoteAgentConfig) -> Result<Box<dyn SandboxProvider>, SandboxError> {
    match config.provider {
        RemoteAgentProvider::E2b => Ok(Box::new(E2BProvider::from_config(config)?)),
        RemoteAgentProvider::Modal => Ok(Box::new(ModalProvider::from_config(config)?)),
        RemoteAgentProvider::Docker => Ok(Box::new(DockerProvider::from_config(config))),
        provider => Err(SandboxError::Unsupported {
            provider: provider.as_str().to_string(),
            message: format!(
                "{} sandbox provisioning is not implemented yet",
                provider.as_str()
            ),
        }),
    }
}
