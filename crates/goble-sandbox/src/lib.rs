//! Profile-driven process isolation.
//!
//! `goble-sandbox` is the isolation seam of the Goble runtime: it turns a
//! [`SandboxProfile`] (an allow-list plus hardened flags at a chosen
//! [`SandboxLevel`]) into a prepared environment that a guarded command runs
//! inside. It is a `runtime`-layer crate in the `types <- protocol <- runtime`
//! split and stays framework-agnostic — no `wgpu`, no `winit`, no UI, no shell
//! harness.
//!
//! The crate is *default-safe* on Linux: [`NoopSandbox`] is the always-built
//! backend and performs no OS isolation (the harness enforces the allow-list
//! itself), so the crate compiles and passes tests everywhere.
//!
//! On macOS, `backends::seatbelt::SeatbeltSandbox` is a *real* backend. It is
//! enabled by default (`unix-seatbelt`): it builds the profile into an `.sb`
//! policy and runs each hardened command under the system `sandbox-exec` launcher,
//! so a hardened profile actually confines the command (and its children) without
//! sandboxing the calling harness/app process. When the launcher is absent it
//! reports [`SandboxError::BackendUnavailable`] rather than pretending to harden.
//!
//! The Linux kernel-call backends — Landlock and seccomp — remain gated behind
//! `unix-*` cargo features, and because the workspace denies `unsafe` by default
//! they declare the seam and report themselves as unavailable for hardened
//! profiles rather than silently pretending to harden. A Linux build opts in to
//! those (and to `unsafe`) to make them real.
//!
//! `BwrapSandbox` (in `backends::bwrap`) is the Linux hardener: it hardens a
//! [`SandboxLevel::Hardened`] profile by spawning the `bwrap` executable with the
//! profile's hardened flags as `bwrap` options. Spawning an external process
//! needs no `unsafe`, so it is always compiled and is selected for hardened
//! profiles on Linux.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod backends;

/// How much isolation a profile demands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    /// No isolation. The command runs unconfined.
    None,
    /// The command's program must appear in the profile's allow-list.
    AllowList,
    /// Full hardening: hardened flags are applied and the environment locked down.
    Hardened,
}

/// The hardened-mode switches. `#[serde(default)]` so an on-the-wire profile only
/// has to carry the flags it wishes to override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HardenedFlags {
    /// Deny outbound network access.
    pub no_network: bool,
    /// Make the workspace filesystem read-only.
    pub read_only_fs: bool,
    /// Refuse to gain new privileges.
    pub no_new_privs: bool,
    /// Disable ptrace / debugging.
    pub disable_ptrace: bool,
}

impl HardenedFlags {
    /// A fully-hardened set of flags, used at [`SandboxLevel::Hardened`].
    pub fn hardened() -> Self {
        Self {
            no_network: true,
            read_only_fs: true,
            no_new_privs: true,
            disable_ptrace: true,
        }
    }

    /// The fully-permissive set, used at [`SandboxLevel::None`].
    pub fn permissive() -> Self {
        Self {
            no_network: false,
            read_only_fs: false,
            no_new_privs: false,
            disable_ptrace: false,
        }
    }
}

impl Default for HardenedFlags {
    fn default() -> Self {
        Self::hardened()
    }
}

fn default_timeout_seconds() -> u64 {
    60
}

/// A named isolation profile: the level, the command allow-list, and the hardened
/// flags to apply. Serializable, so a workspace can ship its policy as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProfile {
    pub name: String,
    pub level: SandboxLevel,
    /// Commands permitted at [`SandboxLevel::AllowList`]. Sorted so serialization
    /// is stable regardless of insertion order.
    #[serde(default)]
    pub allow_list: BTreeSet<String>,
    #[serde(default)]
    pub hardened_flags: HardenedFlags,
    /// Per-command timeout in seconds, applied by the harness-side runner.
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

impl SandboxProfile {
    /// An unconfined profile ([`SandboxLevel::None`]).
    pub fn none(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: SandboxLevel::None,
            allow_list: BTreeSet::new(),
            hardened_flags: HardenedFlags::permissive(),
            timeout_seconds: default_timeout_seconds(),
        }
    }

    /// An allow-list profile ([`SandboxLevel::AllowList`]) permitting `commands`.
    pub fn allow_list(
        name: impl Into<String>,
        commands: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            level: SandboxLevel::AllowList,
            allow_list: commands.into_iter().map(Into::into).collect(),
            hardened_flags: HardenedFlags::permissive(),
            timeout_seconds: default_timeout_seconds(),
        }
    }

    /// A hardened profile ([`SandboxLevel::Hardened`]) with the default hardened flags.
    pub fn hardened(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: SandboxLevel::Hardened,
            allow_list: BTreeSet::new(),
            hardened_flags: HardenedFlags::hardened(),
            timeout_seconds: default_timeout_seconds(),
        }
    }
}

/// A command readied to run inside a sandbox. Builder-style; mirrors the small
/// subset of [`std::process::Command`] a confined runner needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

impl PreparedCommand {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: PathBuf::from("."),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = cwd.into();
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The program's basename, e.g. `cargo` for `/usr/bin/cargo`.
    pub fn basename(&self) -> &str {
        Path::new(&self.program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.program)
    }

    fn to_command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        cmd.current_dir(&self.cwd);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }

    /// Convert to a [`tokio::process::Command`] so the prepared command can be
    /// run asynchronously. Only available when the crate's `tokio` feature is
    /// enabled.
    #[cfg(feature = "tokio")]
    pub fn to_tokio_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args);
        cmd.current_dir(&self.cwd);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
}

/// Errors from sandbox preparation and execution.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("command `{command}` is not in the sandbox allow-list")]
    NotInAllowList { command: String },
    #[error("sandbox profile level {level:?} requires the `{backend}` backend, which is not available on this build")]
    UnsupportedLevel {
        level: SandboxLevel,
        backend: &'static str,
    },
    #[error("sandbox backend `{backend}` is unavailable: {reason}")]
    BackendUnavailable {
        backend: &'static str,
        reason: String,
    },
    #[error("failed to run command: {0}")]
    Io(#[from] std::io::Error),
}

/// Shareable allow-list / level check. Used by [`NoopSandbox`] and the gated OS
/// backends alike. For [`SandboxLevel::Hardened`] it defers to the backend,
/// which decides whether it can actually harden.
pub(crate) fn allowed(profile: &SandboxProfile, command: &PreparedCommand) -> bool {
    match profile.level {
        SandboxLevel::None => !command.program.is_empty(),
        SandboxLevel::AllowList => {
            profile.allow_list.contains(&command.program)
                || profile.allow_list.contains(command.basename())
        }
        SandboxLevel::Hardened => true,
    }
}

/// An object-safe isolation backend. A sandbox is [`prepare`](Sandbox::prepare)d
/// around a command and then [`teardown`](Sandbox::teardown)d when the guarded
/// command finishes. `prepare` applies the profile (checking the allow-list,
/// applying hardened flags, creating OS namespaces); `teardown` releases anything
/// `prepare` acquired.
pub trait Sandbox: Send + Sync {
    /// A short backend name for diagnostics.
    fn name(&self) -> &'static str;

    /// The profile this sandbox was configured with.
    fn profile(&self) -> &SandboxProfile;

    /// Ready the sandbox so `command` can be run confined.
    fn prepare(&self, command: &PreparedCommand) -> Result<(), SandboxError>;

    /// Tear down the sandbox, releasing anything `prepare` acquired.
    fn teardown(&self) -> Result<(), SandboxError>;

    /// The command as it will actually be executed. A hardening backend wraps
    /// `command` (e.g. under `bwrap`) so the guard runs the confined form; the
    /// default returns the command unchanged.
    fn wrap(&self, command: &PreparedCommand) -> PreparedCommand {
        command.clone()
    }
}

/// The always-built, no-op sandbox. It performs no OS isolation: it only checks
/// the profile's allow-list at [`SandboxLevel::AllowList`] and refuses to claim
/// hardening at [`SandboxLevel::Hardened`].
#[derive(Debug, Clone)]
pub struct NoopSandbox {
    profile: SandboxProfile,
}

impl NoopSandbox {
    pub fn new(profile: SandboxProfile) -> Self {
        Self { profile }
    }
}

impl Default for NoopSandbox {
    fn default() -> Self {
        Self::new(SandboxProfile::none("noop"))
    }
}

impl Sandbox for NoopSandbox {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    fn prepare(&self, command: &PreparedCommand) -> Result<(), SandboxError> {
        if self.profile.level == SandboxLevel::Hardened {
            return Err(SandboxError::UnsupportedLevel {
                level: SandboxLevel::Hardened,
                backend: self.name(),
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

/// Instantiate the sandbox backend for `profile`.
///
/// A [`SandboxLevel::Hardened`] profile gets a backend that really confines the
/// command: [`BwrapSandbox`] on Linux (by spawning `bwrap`) and
/// [`SeatbeltSandbox`] on macOS (by applying an `.sb` policy with
/// `sandbox_apply`). A `unix-*` feature enabled on a matching OS may also return
/// that backend for lower levels; otherwise it returns the always-available
/// [`NoopSandbox`], which performs no OS isolation.
pub fn sandbox_for(profile: SandboxProfile) -> Box<dyn Sandbox> {
    #[cfg(target_os = "linux")]
    if profile.level == SandboxLevel::Hardened {
        return Box::new(backends::bwrap::BwrapSandbox::new(profile));
    }
    #[cfg(all(feature = "unix-seatbelt", target_os = "macos"))]
    if profile.level == SandboxLevel::Hardened {
        return Box::new(backends::seatbelt::SeatbeltSandbox::new(profile));
    }
    #[cfg(all(feature = "unix-landlock", target_os = "linux"))]
    {
        return Box::new(backends::landlock::LandlockSandbox::new(profile));
    }
    #[cfg(all(feature = "unix-seccomp", target_os = "linux"))]
    {
        return Box::new(backends::seccomp::SeccompSandbox::new(profile));
    }
    Box::new(NoopSandbox::new(profile))
}

/// A RAII guard that [`prepare`](Sandbox::prepare)s a sandbox for one
/// [`PreparedCommand`], runs it, and [`teardown`](Sandbox::teardown)s the sandbox
/// when it goes out of scope. This guarantees a prepared environment is always
/// released even if the caller forgets to tear it down.
pub struct SandboxGuard<'a> {
    sandbox: &'a dyn Sandbox,
    command: PreparedCommand,
    prepared: bool,
}

impl<'a> SandboxGuard<'a> {
    /// Prepare `sandbox` for `command`. Fails (with a typed [`SandboxError`]) if
    /// the profile rejects the command.
    pub fn new(sandbox: &'a dyn Sandbox, command: PreparedCommand) -> Result<Self, SandboxError> {
        sandbox.prepare(&command)?;
        Ok(Self {
            sandbox,
            command,
            prepared: true,
        })
    }

    /// Run the prepared command and capture its output. A cleanly-run command's
    /// exit status is returned in the [`Output`]; it is not turned into an error.
    ///
    /// A [`SandboxLevel::Hardened`] profile is run through the sandbox wrapper
    /// (e.g. `bwrap`) so the hardened flags actually confine the command;
    /// lower levels run the command directly.
    pub fn run(&mut self) -> anyhow::Result<Output> {
        let wrapped = self.sandbox.wrap(&self.command);
        let out = wrapped.to_command().output()?;
        Ok(out)
    }

    /// Run the prepared command asynchronously and capture its output. A
    /// cleanly-run command's exit status is returned in the [`Output`]; it is
    /// not turned into an error.
    ///
    /// A [`SandboxLevel::Hardened`] profile is run through the sandbox wrapper
    /// (e.g. `bwrap`) so the hardened flags actually confine the command; lower
    /// levels run the command directly. Only available when the crate's `tokio`
    /// feature is enabled.
    #[cfg(feature = "tokio")]
    pub async fn run_async(&mut self) -> anyhow::Result<Output> {
        let wrapped = self.sandbox.wrap(&self.command);
        let out = wrapped.to_tokio_command().output().await?;
        Ok(out)
    }
}

impl Drop for SandboxGuard<'_> {
    fn drop(&mut self) {
        if self.prepared {
            let _ = self.sandbox.teardown();
        }
    }
}

/// Convenience wrapper: prepare `sandbox` for `command`, run it, and tear the
/// sandbox down when the guard drops. Returns the command's [`Output`].
pub fn run_in_sandbox(
    sandbox: &dyn Sandbox,
    command: PreparedCommand,
) -> anyhow::Result<Output> {
    let mut guard = SandboxGuard::new(sandbox, command)?;
    guard.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn profile_builders_set_level_and_defaults() {
        let none = SandboxProfile::none("free");
        assert_eq!(none.level, SandboxLevel::None);
        assert_eq!(none.allow_list.len(), 0);
        assert_eq!(none.hardened_flags, HardenedFlags::permissive());
        assert_eq!(none.timeout_seconds, 60);

        let list = SandboxProfile::allow_list("tools", ["echo", "git", "cargo"]);
        assert_eq!(list.level, SandboxLevel::AllowList);
        assert_eq!(list.allow_list.len(), 3);
        assert!(list.allow_list.contains("cargo"));

        let hard = SandboxProfile::hardened("prod");
        assert_eq!(hard.level, SandboxLevel::Hardened);
        assert_eq!(hard.hardened_flags, HardenedFlags::hardened());
        assert!(hard.hardened_flags.no_network);
        assert!(hard.hardened_flags.read_only_fs);
    }

    #[test]
    fn profile_serializes_roundtrip() {
        let profile = SandboxProfile::allow_list("tools", ["echo", "git"]);
        let json = serde_json::to_string(&profile).unwrap();
        let decoded: SandboxProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, profile);
    }

    #[test]
    fn profile_deserializes_missing_defaulted_fields() {
        let json = r#"{ "name": "minimal", "level": "allow_list" }"#;
        let decoded: SandboxProfile = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.level, SandboxLevel::AllowList);
        assert!(decoded.allow_list.is_empty());
        assert_eq!(decoded.hardened_flags, HardenedFlags::hardened());
        assert_eq!(decoded.timeout_seconds, 60);
    }

    #[test]
    fn level_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&SandboxLevel::None).unwrap(), "\"none\"");
        assert_eq!(
            serde_json::to_string(&SandboxLevel::AllowList).unwrap(),
            "\"allow_list\""
        );
        assert_eq!(
            serde_json::to_string(&SandboxLevel::Hardened).unwrap(),
            "\"hardened\""
        );
    }

    #[test]
    fn noop_prepares_none_level() {
        let sb = NoopSandbox::new(SandboxProfile::none("free"));
        let cmd = PreparedCommand::new("echo").arg("hi");
        assert!(sb.prepare(&cmd).is_ok());
    }

    #[test]
    fn noop_enforces_allow_list() {
        let sb = NoopSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        let ok = PreparedCommand::new("echo").arg("hi");
        assert!(sb.prepare(&ok).is_ok());

        let bad = PreparedCommand::new("rm").arg("-rf").arg("/");
        assert!(matches!(
            sb.prepare(&bad),
            Err(SandboxError::NotInAllowList { command }) if command == "rm"
        ));
    }

    #[test]
    fn noop_allows_by_basename_for_full_path() {
        let sb = NoopSandbox::new(SandboxProfile::allow_list("tools", ["cargo"]));
        let cmd = PreparedCommand::new("/usr/bin/cargo").arg("build");
        assert!(sb.prepare(&cmd).is_ok());
    }

    #[test]
    fn noop_rejects_hardened() {
        let sb = NoopSandbox::new(SandboxProfile::hardened("prod"));
        let cmd = PreparedCommand::new("echo");
        assert!(matches!(
            sb.prepare(&cmd),
            Err(SandboxError::UnsupportedLevel { level, backend }) if level == SandboxLevel::Hardened && backend == "noop"
        ));
    }

    #[test]
    fn noop_teardown_is_ok() {
        let sb = NoopSandbox::default();
        assert!(sb.teardown().is_ok());
        assert_eq!(sb.name(), "noop");
    }

    #[test]
    fn guard_runs_a_prepared_command() {
        let sb = NoopSandbox::new(SandboxProfile::none("free"));
        let cmd = PreparedCommand::new("echo").arg("hello");
        let mut guard = SandboxGuard::new(&sb, cmd).unwrap();
        let out = guard.run().unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
    }

    #[test]
    fn guard_fails_on_disallowed_command() {
        let sb = NoopSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        let cmd = PreparedCommand::new("rm");
        assert!(matches!(
            SandboxGuard::new(&sb, cmd),
            Err(SandboxError::NotInAllowList { .. })
        ));
    }

    /// A sandbox that records whether it was prepared/torn down, to prove the RAII
    /// guard always releases the environment.
    struct TrackingSandbox {
        profile: SandboxProfile,
        prepared: Arc<AtomicBool>,
        torn: Arc<AtomicBool>,
    }

    impl TrackingSandbox {
        fn new(profile: SandboxProfile) -> (Self, Arc<AtomicBool>, Arc<AtomicBool>) {
            let prepared = Arc::new(AtomicBool::new(false));
            let torn = Arc::new(AtomicBool::new(false));
            (
                Self {
                    profile,
                    prepared: prepared.clone(),
                    torn: torn.clone(),
                },
                prepared,
                torn,
            )
        }
    }

    impl Sandbox for TrackingSandbox {
        fn name(&self) -> &'static str {
            "tracking"
        }
        fn profile(&self) -> &SandboxProfile {
            &self.profile
        }
        fn prepare(&self, command: &PreparedCommand) -> Result<(), SandboxError> {
            self.prepared.store(true, Ordering::SeqCst);
            allowed(&self.profile, command).then_some(()).ok_or_else(|| {
                SandboxError::NotInAllowList {
                    command: command.basename().to_string(),
                }
            })
        }
        fn teardown(&self) -> Result<(), SandboxError> {
            self.torn.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn guard_tears_down_on_drop_even_without_run() {
        let (sb, prepared, torn) = TrackingSandbox::new(SandboxProfile::none("track"));
        {
            let _guard = SandboxGuard::new(&sb, PreparedCommand::new("echo")).unwrap();
            assert!(prepared.load(Ordering::SeqCst));
            assert!(!torn.load(Ordering::SeqCst));
        }
        // Guard dropped: teardown ran.
        assert!(torn.load(Ordering::SeqCst));
    }

    #[test]
    fn guard_tears_down_after_run() {
        let (sb, _prepared, torn) = TrackingSandbox::new(SandboxProfile::none("track"));
        {
            let mut guard = SandboxGuard::new(&sb, PreparedCommand::new("echo").arg("x")).unwrap();
            let _ = guard.run().unwrap();
            assert!(!torn.load(Ordering::SeqCst));
        }
        assert!(torn.load(Ordering::SeqCst));
    }

    #[test]
    fn run_in_sandbox_convenience() {
        let sb = NoopSandbox::new(SandboxProfile::none("free"));
        let out = run_in_sandbox(&sb, PreparedCommand::new("echo").arg("hi")).unwrap();
        assert!(out.status.success());
    }

    #[test]
    fn default_sandbox_for_is_noop() {
        let sb = sandbox_for(SandboxProfile::none("default"));
        // Without a matching os/feature backend the default is the no-op.
        assert_eq!(sb.name(), "noop");
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn guard_runs_a_prepared_command_async() {
        let sb = NoopSandbox::new(SandboxProfile::none("free"));
        let cmd = PreparedCommand::new("echo").arg("async-hello");
        let mut guard = SandboxGuard::new(&sb, cmd).unwrap();
        let out = guard.run_async().await.unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("async-hello"));
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn guard_async_fails_on_disallowed_command() {
        let sb = NoopSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        let cmd = PreparedCommand::new("rm");
        assert!(matches!(
            SandboxGuard::new(&sb, cmd),
            Err(SandboxError::NotInAllowList { .. })
        ));
    }
}
