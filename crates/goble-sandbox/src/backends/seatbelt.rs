#![allow(unsafe_code)]
//! Apple Seatbelt backend (`sandbox-exec`).
//!
//! This is a *real* macOS hardener. It compiles the profile's `.sb` policy (see
//! [`build_profile`]) and runs the command inside a seatbelt profile by spawning
//! the system `sandbox-exec` launcher (the `wrap` hook), which confines only the
//! command — and its children — rather than the calling process. Unlike the
//! gated Linux backends, a hardened profile is genuinely enforced on macOS.
//!
//! The harness profile keeps the workspace writable (`read_only_fs` is `false`)
//! so `write_file`/`edit_file`/`delete_file`/`git_commit` keep working: reads are
//! allowed broadly (so the toolchain and workspace are readable), writes are
//! confined to the workspace subpath, and network is denied unless the profile
//! permits it. Confinement is applied in a child process, so the running harness
//! (which makes the model API calls) is never sandboxed itself.
//!
//! `prepare` only checks the allow-list and that the launcher exists — it never
//! applies the sandbox to the current process, so probing availability
//! ([`crate::harness_sandbox`]) does not confine the caller. The real
//! `sandbox_apply` FFI path is kept behind a test gate so an opt-in child process
//! can prove the generated profile actually denies writes.

use crate::{allowed, PreparedCommand, Sandbox, SandboxError, SandboxLevel, SandboxProfile};
use std::path::{Path, PathBuf};

/// The macOS `sandbox-exec` launcher that confines a child command.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// The Apple Seatbelt sandbox backend.
pub struct SeatbeltSandbox {
    profile: SandboxProfile,
}

impl SeatbeltSandbox {
    pub fn new(profile: SandboxProfile) -> Self {
        Self { profile }
    }

    /// Whether the `sandbox-exec` launcher is present; when it is not, a hardened
    /// profile cannot be enforced and the backend reports itself unavailable.
    fn launcher_available() -> bool {
        Path::new(SANDBOX_EXEC).is_file()
    }
}

impl Sandbox for SeatbeltSandbox {
    fn name(&self) -> &'static str {
        "seatbelt"
    }

    fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    fn prepare(&self, command: &PreparedCommand) -> Result<(), SandboxError> {
        if !allowed(&self.profile, command) {
            return Err(SandboxError::NotInAllowList {
                command: command.basename().to_string(),
            });
        }
        if self.profile.level == SandboxLevel::Hardened {
            if !Self::launcher_available() {
                return Err(SandboxError::BackendUnavailable {
                    backend: self.name(),
                    reason: format!("{SANDBOX_EXEC} is not installed; cannot apply a seatbelt profile"),
                });
            }
        }
        Ok(())
    }

    fn teardown(&self) -> Result<(), SandboxError> {
        // Seatbelt confinement is per-child (the launcher applies it); there is
        // nothing to release on the calling process.
        Ok(())
    }

    fn wrap(&self, command: &PreparedCommand) -> PreparedCommand {
        if self.profile.level != SandboxLevel::Hardened {
            return command.clone();
        }
        // `sandbox-exec -p <profile> <program> <args...>` confines only the child
        // it execs, so the harness process (and its model calls) stay unconfined.
        let profile = build_profile(&self.profile, command);
        let mut args = vec!["-p".to_string(), profile];
        args.push(command.program.clone());
        args.extend(command.args.iter().cloned());
        let mut wrapped = PreparedCommand::new(SANDBOX_EXEC);
        wrapped = wrapped.args(args);
        wrapped.cwd = command.cwd.clone();
        wrapped.env = command.env.clone();
        wrapped
    }
}

// --- Pure profile construction (tested without the kernel) -------------------

/// Normalize the write-allowable workspace path.
///
/// Seatbelt `(subpath "…")` rules must match the real on-disk path. `canonicalize`
/// resolves symlinks so a `(subpath "/var/…")` matches `/private/var/…` on macOS,
/// and an absolute fallback keeps the rule usable when the path does not yet
/// exist.
fn normalize_workspace(cwd: &Path) -> PathBuf {
    if cwd.as_os_str().is_empty() {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    }
    if let Ok(canonical) = std::fs::canonicalize(cwd) {
        return canonical;
    }
    if let Ok(absolute) = std::path::absolute(cwd) {
        return absolute;
    }
    cwd.to_path_buf()
}

/// One Seatbelt rule: `(allow|deny <operation>[ (subpath "<path>")])`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SbRule {
    pub allow: bool,
    pub operation: &'static str,
    pub subpath: Option<String>,
}

impl SbRule {
    fn allow(operation: &'static str) -> Self {
        Self {
            allow: true,
            operation,
            subpath: None,
        }
    }

    fn deny(operation: &'static str) -> Self {
        Self {
            allow: false,
            operation,
            subpath: None,
        }
    }

    fn allow_on(operation: &'static str, path: &Path) -> Self {
        Self {
            allow: true,
            operation,
            subpath: Some(path.to_string_lossy().into_owned()),
        }
    }
}

/// The allow/deny decision table: the ordered Seatbelt rules implied by
/// `profile` for a workspace rooted at `workspace`.
///
/// The workspace is writable when `read_only_fs` is false (the harness profile)
/// and read-only otherwise; reads are allowed broadly so the confined command can
/// load its executable and libraries; network is denied by default and only
/// allowed when the profile lifts the restriction.
pub(crate) fn rules_for(profile: &SandboxProfile, workspace: &Path) -> Vec<SbRule> {
    let flags = profile.hardened_flags;
    let mut rules = Vec::new();

    // The command must be able to launch and manage its children, and the
    // process needs a few syscalls to bootstrap.
    rules.push(SbRule::allow("process*"));
    rules.push(SbRule::allow("file-read-metadata"));
    rules.push(SbRule::allow("sysctl-read"));
    rules.push(SbRule::allow("mach-lookup"));
    rules.push(SbRule::allow("ipc-posix-shm"));

    // Reads: the workspace and toolchain must be readable so the confined
    // command can execute and inspect its inputs.
    rules.push(SbRule::allow("file-read*"));
    rules.push(SbRule::allow_on("file-read*", workspace));

    // Writes are confined to the workspace. When the profile keeps the workspace
    // writable (`read_only_fs` false, as the harness does so the file/git tools
    // keep working) a path-scoped allow overrides the broad deny; otherwise the
    // workspace is read-only and every write is denied.
    rules.push(SbRule::deny("file-write*"));
    if !flags.read_only_fs {
        rules.push(SbRule::allow_on("file-write*", workspace));
    }

    // Network is denied by default; only allowed when the profile lifts it.
    if flags.no_network {
        rules.push(SbRule::deny("network*"));
    } else {
        rules.push(SbRule::allow("network*"));
    }

    rules
}

fn render_rule(rule: &SbRule) -> String {
    let action = if rule.allow { "allow" } else { "deny" };
    let mut line = format!("({action} {}", rule.operation);
    if let Some(subpath) = &rule.subpath {
        line.push_str(&format!(" (subpath \"{subpath}\")"));
    }
    line.push_str(")\n");
    line
}

/// Build a Seatbelt `.sb` profile string for `profile` and `command`.
pub(crate) fn build_profile(profile: &SandboxProfile, command: &PreparedCommand) -> String {
    let workspace = normalize_workspace(&command.cwd);
    let mut sb = String::from("(version 1)\n(deny default)\n");
    for rule in rules_for(profile, &workspace) {
        sb.push_str(&render_rule(&rule));
    }
    sb
}

// --- FFI (macOS only, test-only) ---------------------------------------------
//
// The runtime hardener is `sandbox-exec`; this FFI path exists only so an
// opt-in child process can prove the compiled profile truly denies writes via
// `sandbox_apply` (see the probe tests below). It is compiled only under `test`
// to avoid dead-code warnings in production builds.

#[cfg(all(test, target_os = "macos"))]
mod ffi {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int, c_void};

    // The private `libsandbox` dylib ships the string-compilation API that the
    // public `sandbox.h` (which only declares the named-profile `sandbox_init`)
    // does not.
    #[link(name = "sandbox")]
    extern "C" {
        fn sandbox_compile_string(profile: *const c_char, bufp: *mut *mut c_void) -> c_int;
        fn sandbox_apply(profile: *mut c_void) -> c_int;
        fn sandbox_free_profile(profile: *mut c_void);
    }

    /// Compile `profile` and apply it to the current process.
    pub(crate) fn compile_and_apply(profile: &CStr) -> Result<(), String> {
        let mut handle: *mut c_void = std::ptr::null_mut();
        // SAFETY: `handle` is a valid, null-initialized slot. On a 0 return the
        // kernel writes a heap-allocated compiled profile into it, which we own.
        let ret = unsafe { sandbox_compile_string(profile.as_ptr(), &mut handle) };
        if ret != 0 || handle.is_null() {
            return Err(format!("sandbox_compile_string failed (errno {ret})"));
        }
        // SAFETY: `handle` is a valid compiled profile from sandbox_compile_string.
        let apply_ret = unsafe { sandbox_apply(handle) };
        // SAFETY: `handle` is freed exactly once and by the only consumer.
        unsafe { sandbox_free_profile(handle) };
        if apply_ret != 0 {
            return Err(format!("sandbox_apply failed (errno {apply_ret})"));
        }
        Ok(())
    }
}

/// Apply a hardened profile to the current process via `sandbox_apply`. This is
/// irreversible, so it is only ever called by an opt-in child process in tests.
#[cfg(all(test, target_os = "macos"))]
fn apply_compiled(profile: &std::ffi::CStr) -> Result<(), SandboxError> {
    ffi::compile_and_apply(profile).map_err(|reason| SandboxError::BackendUnavailable {
        backend: "seatbelt",
        reason,
    })
}

// --- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The harness profile: workspace kept writable so the file/git tools work.
    fn harness_profile() -> SandboxProfile {
        let mut profile = SandboxProfile::hardened("harness");
        profile.hardened_flags.read_only_fs = false;
        profile
    }

    fn command_cwd(cwd: &Path) -> PreparedCommand {
        PreparedCommand::new("/usr/bin/echo").arg("hi").cwd(cwd)
    }

    #[test]
    fn hardened_profile_builds_a_nontrivial_sb_string() {
        let profile = harness_profile();
        let sb = build_profile(&profile, &command_cwd(Path::new("/workspace")));

        assert!(sb.starts_with("(version 1)\n(deny default)\n"));
        assert!(sb.contains("(allow file-read*)"));
        assert!(sb.contains("(deny file-write*)"));
        assert!(sb.contains("(allow file-write* (subpath \"/workspace\"))"));
        assert!(sb.contains("(deny network*)"));
        assert!(sb.contains("(allow process*)"));
    }

    #[test]
    fn hardened_profile_resolves_macos_symlinked_workspace() {
        let profile = harness_profile();
        let sb = build_profile(&profile, &command_cwd(Path::new("/var/goble/ws")));
        let resolved = normalize_workspace(Path::new("/var/goble/ws"));
        assert!(resolved.starts_with("/private/var/") || resolved.starts_with("/var/"));
        assert!(sb.contains(&format!("(allow file-write* (subpath \"{}\"))", resolved.display())));
    }

    #[test]
    fn read_only_profile_denies_all_writes() {
        let profile = SandboxProfile::hardened("prod"); // read_only_fs defaults true
        let rules = rules_for(&profile, Path::new("/workspace"));
        assert!(rules.iter().any(|r| !r.allow && r.operation == "file-write*"));
        assert!(!rules.iter().any(|r| r.allow && r.operation == "file-write*"));
    }

    #[test]
    fn decision_table_network_follows_no_network_flag() {
        let mut denied = harness_profile();
        denied.hardened_flags.no_network = true;
        let mut allowed_net = harness_profile();
        allowed_net.hardened_flags.no_network = false;

        let d = rules_for(&denied, Path::new("/ws"));
        assert!(d.iter().any(|r| !r.allow && r.operation == "network*"));
        assert!(!d.iter().any(|r| r.allow && r.operation == "network*"));

        let a = rules_for(&allowed_net, Path::new("/ws"));
        assert!(a.iter().any(|r| r.allow && r.operation == "network*"));
        assert!(!a.iter().any(|r| !r.allow && r.operation == "network*"));
    }

    #[test]
    fn wrap_hardened_prepends_the_launcher() {
        let sb = SeatbeltSandbox::new(harness_profile());
        let cmd = command_cwd(Path::new("/workspace"));
        let wrapped = sb.wrap(&cmd);
        assert_eq!(wrapped.program, SANDBOX_EXEC);
        // `-p <profile>` first, then the original program and its args.
        assert_eq!(wrapped.args.first().map(String::as_str), Some("-p"));
        assert!(wrapped.args.len() >= 2);
        assert!(wrapped.args.windows(2).any(|w| w[1] == "/usr/bin/echo"));
        assert_eq!(wrapped.cwd, cmd.cwd);
        for (k, v) in &cmd.env {
            assert!(wrapped.env.contains(&(k.clone(), v.clone())));
        }
    }

    #[test]
    fn wrap_leaves_non_hardened_command_unchanged() {
        let sb = SeatbeltSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        let cmd = command_cwd(Path::new("/workspace"));
        assert_eq!(sb.wrap(&cmd), cmd);
    }

    #[test]
    fn prepare_requires_the_launcher_for_hardened() {
        let sb = SeatbeltSandbox::new(harness_profile());
        match sb.prepare(&PreparedCommand::new("echo").cwd("/workspace")) {
            // On this macOS host `sandbox-exec` is present, so a hardened profile
            // is usable.
            Ok(()) => assert!(SeatbeltSandbox::launcher_available()),
            Err(SandboxError::BackendUnavailable { .. }) => assert!(!SeatbeltSandbox::launcher_available()),
            other => panic!("unexpected prepare result: {other:?}"),
        }
    }

    #[test]
    fn prepare_enforces_allow_list_at_allow_list_level() {
        // At AllowList level the sandbox rejects a command outside the list. (A
        // Hardened profile deliberately skips the allow-list — `allowed()`
        // returns true — leaving the runner's `allowed_commands` gate as the real
        // command allow-list.)
        let sb = SeatbeltSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        assert!(sb.prepare(&PreparedCommand::new("echo")).is_ok());
        assert!(matches!(
            sb.prepare(&PreparedCommand::new("rm")),
            Err(SandboxError::NotInAllowList { .. })
        ));
    }

    #[test]
    fn prepare_of_non_hardened_does_not_need_the_launcher() {
        let sb = SeatbeltSandbox::new(SandboxProfile::allow_list("tools", ["echo"]));
        assert!(sb.prepare(&PreparedCommand::new("echo")).is_ok());
    }

    #[test]
    fn seatbelt_teardown_is_ok() {
        let sb = SeatbeltSandbox::new(harness_profile());
        assert!(sb.teardown().is_ok());
        assert_eq!(sb.name(), "seatbelt");
    }

    // An irreversible `sandbox_apply` will poison the calling process, so the
    // real-apply path is exercised only in an opt-in child process.
    #[test]
    fn seatbelt_probe_applies_when_requested() {
        if std::env::var_os("GOBLE_SEATBELT_PROBE").is_none() {
            eprintln!(
                "skip seatbelt probe: set GOBLE_SEATBELT_PROBE=1 to exercise the real sandbox_apply"
            );
            return;
        }
        let exe = std::env::current_exe().expect("current_exe");
        let status = std::process::Command::new(exe)
            .arg("--exact")
            .arg("seatbelt_probe_child")
            .env("GOBLE_SEATBELT_PROBE_CHILD", "1")
            .status()
            .expect("spawn seatbelt probe child");
        assert!(status.success(), "seatbelt probe child failed");
    }

    // Runs only when launched by `seatbelt_probe_applies_when_requested`, so it
    // never runs the real `sandbox_apply` during an ordinary `cargo test`.
    #[test]
    fn seatbelt_probe_child() {
        if std::env::var_os("GOBLE_SEATBELT_PROBE_CHILD").is_none() {
            return;
        }
        let profile = SandboxProfile::hardened("probe");
        let command = command_cwd(&std::env::current_dir().expect("cwd"));
        let sb = build_profile(&profile, &command);
        let cstr = std::ffi::CString::new(sb).expect("no NUL");
        // Apply the compiled profile to this child process so the write below is
        // actually confined; an irreversible `sandbox_apply` is why this only runs
        // inside an opt-in child process.
        apply_compiled(&cstr).expect("applying the seatbelt profile should succeed");

        // A write outside the workspace's allowed subpath must be denied by the
        // `(deny file-write*)` rule. `std::env::temp_dir()` resolves through the
        // /var -> /private/var symlink, which is not the workspace subpath.
        let forbidden = std::env::temp_dir().join(format!("goble_seatbelt_probe_{}.tmp", std::process::id()));
        let write_result = std::fs::write(&forbidden, b"should be denied");
        assert!(write_result.is_err(), "sandbox allowed a write outside the workspace");
        let _ = std::fs::remove_file(&forbidden);
    }
}
