//! Linux bubblewrap backend.
//!
//! This is the one backend in the crate that *actually* hardens a
//! [`SandboxLevel::Hardened`] profile. It expresses the profile's hardened
//! flags as options to the `bwrap` executable (bubblewrap). Spawning an
//! external process needs no `unsafe`, so it is not blocked by the workspace's
//! `unsafe_code = "deny"` lint and is always compiled, unlike the kernel-call
//! backends (Landlock, seccomp, Seatbelt) that would require raw syscalls.
//!
//! Bubblewrap cannot express every hardened flag directly:
//!   * `disable_ptrace` needs a seccomp BPF program, which we cannot build
//!     without `unsafe`; it is therefore only best-effort and not passed
//!     through to `bwrap`.

use crate::{allowed, PreparedCommand, Sandbox, SandboxError, SandboxLevel, SandboxProfile};

/// The Linux bubblewrap backend. A [`SandboxLevel::Hardened`] profile is run
/// through the `bwrap` executable with the profile's hardened flags as `bwrap`
/// options; [`SandboxLevel::AllowList`] profiles get the same allow-list check
/// as [`crate::NoopSandbox`].
pub struct BwrapSandbox {
    profile: SandboxProfile,
}

// On non-Linux this backend is still compiled so its pure arg-construction unit
// tests run everywhere, but `sandbox_for` never selects it, so its inherent
// members are unused there. Suppress that cross-platform false positive; on
// Linux the members are genuinely used and stay checked.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl BwrapSandbox {
    pub fn new(profile: SandboxProfile) -> Self {
        Self { profile }
    }

    /// The `bwrap` command that runs `command` inside a hardened bubblewrap
    /// sandbox, carrying the profile's hardened flags as `bwrap` options.
    fn build_wrapped(&self, command: &PreparedCommand) -> PreparedCommand {
        let flags = &self.profile.hardened_flags;
        let mut bwrap = PreparedCommand::new("bwrap");

        if flags.no_network {
            bwrap = bwrap.arg("--unshare-net");
        }
        // Bubblewrap starts from an empty mount tree, so the workspace (the
        // command's cwd) must be bound in for the command to see it. `read_only_fs`
        // makes it read-only (`--ro-bind`); when the profile keeps the workspace
        // writable (`read_only_fs` is false, as the harness profile does so
        // write_file/edit_file/delete_file/rename_file/git_commit keep working)
        // we bind it read-write (`--bind`). Everything outside the workspace is
        // left isolated by the empty mount tree.
        let cwd = command.cwd.to_string_lossy().into_owned();
        if flags.read_only_fs {
            bwrap = bwrap.arg("--ro-bind").arg(cwd.clone()).arg(cwd);
        } else if !cwd.is_empty() {
            bwrap = bwrap.arg("--bind").arg(cwd.clone()).arg(cwd);
        }
        if flags.no_new_privs {
            // Bubblewrap has no direct `no_new_privs` option. `--die-with-parent`
            // and a private PID namespace (`--unshare-pid`) are the closest
            // expressions of refusing to gain privileges.
            bwrap = bwrap.arg("--die-with-parent").arg("--unshare-pid");
        }
        // `disable_ptrace` is not expressible: it would need a seccomp BPF
        // program, which `unsafe_code = "deny"` prevents us from building here.
        // It is best-effort only.

        // Everything after `--` is the program to run inside the sandbox.
        bwrap = bwrap.arg("--");
        bwrap = bwrap.arg(&command.program);
        bwrap = bwrap.args(&command.args);
        bwrap = bwrap.cwd(&command.cwd);
        for (k, v) in &command.env {
            bwrap = bwrap.env(k.clone(), v.clone());
        }
        bwrap
    }
}

impl Sandbox for BwrapSandbox {
    fn name(&self) -> &'static str {
        "bwrap"
    }

    fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    fn prepare(&self, command: &PreparedCommand) -> Result<(), SandboxError> {
        // The allow-list is enforced at [SandboxLevel::AllowList].
        if !allowed(&self.profile, command) {
            return Err(SandboxError::NotInAllowList {
                command: command.basename().to_string(),
            });
        }
        // A hardened profile is supported here: build the wrapping `bwrap`
        // command now so any construction problem surfaces at prepare time.
        if self.profile.level == SandboxLevel::Hardened {
            let _ = self.build_wrapped(command);
        }
        Ok(())
    }

    fn wrap(&self, command: &PreparedCommand) -> PreparedCommand {
        if self.profile.level == SandboxLevel::Hardened {
            self.build_wrapped(command)
        } else {
            command.clone()
        }
    }

    fn teardown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HardenedFlags, SandboxProfile};

    fn cargo() -> PreparedCommand {
        PreparedCommand::new("/usr/bin/cargo").arg("build").cwd("/workspace")
    }

    fn wraps(sb: &BwrapSandbox, cmd: &PreparedCommand) -> Vec<String> {
        sb.wrap(cmd).args
    }

    #[test]
    fn hardened_profile_wraps_under_bwrap() {
        let sb = BwrapSandbox::new(SandboxProfile::hardened("prod"));
        let wrapped = sb.wrap(&cargo());
        assert_eq!(wrapped.program, "bwrap");
    }

    #[test]
    fn hardened_profile_carries_hardened_flags() {
        let sb = BwrapSandbox::new(SandboxProfile::hardened("prod"));
        let args = wraps(&sb, &cargo());

        assert!(args.contains(&"--unshare-net".to_string()));

        let cwd = "/workspace".to_string();
        assert!(args
            .windows(3)
            .any(|w| w[0] == "--ro-bind" && w[1] == cwd && w[2] == cwd));

        assert!(args.contains(&"--die-with-parent".to_string()));
        assert!(args.contains(&"--unshare-pid".to_string()));
    }

    #[test]
    fn hardened_profile_runs_inner_command_after_separator() {
        let sb = BwrapSandbox::new(SandboxProfile::hardened("prod"));
        let args = wraps(&sb, &cargo());

        let sep = args.iter().position(|a| a == "--").expect("bwrap separator");
        assert_eq!(args[sep + 1], "/usr/bin/cargo");
        assert_eq!(args[sep + 2], "build");
    }

    #[test]
    fn only_enabled_flags_are_emitted() {
        let mut profile = SandboxProfile::hardened("prod");
        profile.hardened_flags = HardenedFlags {
            no_network: true,
            read_only_fs: false,
            no_new_privs: false,
            disable_ptrace: false,
        };
        let sb = BwrapSandbox::new(profile);
        let args = wraps(&sb, &cargo());

        assert!(args.contains(&"--unshare-net".to_string()));
        assert!(!args.contains(&"--die-with-parent".to_string()));
        assert!(!args.contains(&"--unshare-pid".to_string()));
        assert!(!args.iter().any(|a| a == "--ro-bind"));
    }

    #[test]
    fn readwrite_profile_binds_workspace_writable() {
        // The harness profile keeps the workspace writable (`read_only_fs =
        // false`): the workspace must be bind-mounted read-write, not read-only.
        let mut profile = SandboxProfile::hardened("harness");
        profile.hardened_flags.read_only_fs = false;
        let sb = BwrapSandbox::new(profile);
        let args = wraps(&sb, &cargo());

        let cwd = "/workspace".to_string();
        assert!(args
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == cwd && w[2] == cwd));
        assert!(!args.iter().any(|a| a == "--ro-bind"));
    }

    #[test]
    fn non_hardened_profile_is_not_wrapped() {
        let sb = BwrapSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        let wrapped = sb.wrap(&PreparedCommand::new("echo").arg("hi"));
        assert_eq!(wrapped.program, "echo");
        assert_eq!(wrapped.args, vec!["hi".to_string()]);
    }

    #[test]
    fn hardened_profile_prepares_ok() {
        let sb = BwrapSandbox::new(SandboxProfile::hardened("prod"));
        assert!(sb.prepare(&cargo()).is_ok());
    }

    #[test]
    fn bwrap_enforces_allow_list() {
        let sb = BwrapSandbox::new(SandboxProfile::allow_list("tools", ["cargo"]));
        assert!(sb.prepare(&cargo()).is_ok());

        let bad = PreparedCommand::new("rm").arg("-rf").arg("/");
        assert!(matches!(
            sb.prepare(&bad),
            Err(SandboxError::NotInAllowList { command }) if command == "rm"
        ));
    }

    #[test]
    fn bwrap_teardown_is_ok() {
        let sb = BwrapSandbox::new(SandboxProfile::hardened("prod"));
        assert!(sb.teardown().is_ok());
        assert_eq!(sb.name(), "bwrap");
    }
}
