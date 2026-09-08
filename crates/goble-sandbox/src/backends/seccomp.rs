//! Linux seccomp backend.

use crate::{allowed, PreparedCommand, Sandbox, SandboxError, SandboxProfile};

/// The Linux seccomp backend. Declares the isolation seam; hardening a hardened
/// profile is left to a build that opts in to `unsafe` syscalls.
pub struct SeccompSandbox {
    profile: SandboxProfile,
}

impl SeccompSandbox {
    pub fn new(profile: SandboxProfile) -> Self {
        Self { profile }
    }
}

impl Sandbox for SeccompSandbox {
    fn name(&self) -> &'static str {
        "seccomp"
    }

    fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    fn prepare(&self, command: &PreparedCommand) -> Result<(), SandboxError> {
        if self.profile.level == crate::SandboxLevel::Hardened {
            return Err(SandboxError::BackendUnavailable {
                backend: self.name(),
                reason: "seccomp filtering requires `unsafe` syscalls, which the workspace denies"
                    .to_string(),
            });
        }
        if allowed(&self.profile, command) {
            Ok(())
        } else {
            Err(SandboxError::NotInAllowList {
                command: command.basename().to_string(),
            })
        }
    }

    fn teardown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}
