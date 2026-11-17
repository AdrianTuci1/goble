//! Detecting the agent CLIs the catalog names, and probing their version.
//!
//! Settings -> Providers' "Agent CLIs" rows show whether a harness is installed
//! on this machine, where it resolves and what version it reports. Goble never
//! downloads or installs an agent CLI (that is an honest flag in the UI): it
//! resolves the configured command — or the catalog's first command — on `PATH`,
//! and runs `<command> --version` with a [`VERSION_PROBE_TIMEOUT`] timeout.
//!
//! The detection is synchronous and cheap ([`resolve_harness`] /
//! [`status`]); only the version probe is async and is called on an explicit
//! "Test"/refresh, never on startup.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::HarnessConfig;

use super::catalog::HarnessSpec;

/// How long `--version` may take before it is reported as a timeout.
pub const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// One harness as the settings row reads it: the catalog identity, the
/// configured overrides, and what detection found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HarnessStatus {
    pub id: String,
    pub name: String,
    /// Whether a command for this harness resolves on `PATH`.
    pub installed: bool,
    /// The resolved command's full path.
    pub path: Option<String>,
    /// What `--version` printed, when probed.
    pub version: Option<String>,
    /// The command that will be launched (configured override or catalog first).
    pub command: String,
    /// The configured launch arguments.
    pub args: Vec<String>,
    /// Whether the harness is offered at all (`[harness.<id>] enabled`).
    pub enabled: bool,
    /// Whether the file carries a `[harness.<id>]` override.
    pub configured: bool,
    /// Why a probe failed, when it did.
    pub error: Option<String>,
}

impl HarnessStatus {
    /// The pill a row shows: installed, not found, or disabled.
    pub fn status_label(&self) -> &'static str {
        if !self.enabled {
            "disabled"
        } else if self.installed {
            "installed"
        } else {
            "not found"
        }
    }
}

/// The directories `PATH` names, in order. An empty or missing `PATH` names
/// nothing, so detection reports "not found" instead of scanning the CWD.
pub fn path_dirs() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect()
}

/// Resolve a command on the process `PATH`.
pub fn resolve_command(command: &str) -> Option<PathBuf> {
    resolve_command_in(command, &path_dirs())
}

/// Resolve a command in an explicit list of directories. A command that names a
/// path (contains a separator) is checked directly.
pub fn resolve_command_in(command: &str, path_dirs: &[PathBuf]) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    if command.contains('/') || (cfg!(windows) && command.contains('\\')) {
        let path = Path::new(command);
        return is_executable_file(path).then(|| path.to_path_buf());
    }
    for dir in path_dirs {
        for name in executable_names(command) {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_names(command: &str) -> Vec<String> {
    vec![
        command.to_string(),
        format!("{command}.exe"),
        format!("{command}.cmd"),
        format!("{command}.bat"),
    ]
}

#[cfg(not(windows))]
fn executable_names(command: &str) -> Vec<String> {
    vec![command.to_string()]
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The command path a harness launches: the `[harness.<id>] command` override
/// when set, otherwise the first command of the catalog entry that resolves.
pub fn resolve_harness(spec: &HarnessSpec, config: Option<&HarnessConfig>) -> Option<PathBuf> {
    if let Some(override_command) = config
        .and_then(|config| config.command.as_deref())
        .map(str::trim)
        .filter(|command| !command.is_empty())
    {
        return resolve_command(override_command);
    }
    spec.commands
        .iter()
        .find_map(|command| resolve_command(command))
}

/// The command string a harness launches: the configured override, else the
/// catalog's first command, else the harness id.
pub fn launch_command(spec: &HarnessSpec, config: Option<&HarnessConfig>) -> String {
    config
        .and_then(|config| config.command.clone())
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
        .or_else(|| spec.commands.first().map(|command| command.to_string()))
        .unwrap_or_else(|| spec.id.to_string())
}

/// Detect one harness without running anything (the cheap status a settings
/// refresh uses). [`probe_version`] fills the version in on demand.
pub fn status(spec: &HarnessSpec, config: Option<&HarnessConfig>) -> HarnessStatus {
    let path = resolve_harness(spec, config);
    HarnessStatus {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        installed: path.is_some(),
        path: path.map(|path| path.display().to_string()),
        version: None,
        command: launch_command(spec, config),
        args: config.map(|config| config.args.clone()).unwrap_or_default(),
        enabled: config.map(|config| config.enabled).unwrap_or(true),
        configured: config.is_some(),
        error: None,
    }
}

/// Detect one harness and probe its version. A probe failure is kept on the
/// status (`error`) rather than dropped, so a row can say why "installed" has no
/// version.
pub async fn status_with_version(
    spec: &HarnessSpec,
    config: Option<&HarnessConfig>,
) -> HarnessStatus {
    let mut status = status(spec, config);
    let Some(path) = status.path.clone() else {
        return status;
    };
    match probe_version(Path::new(&path)).await {
        Ok(version) => status.version = Some(version),
        Err(error) => status.error = Some(error),
    }
    status
}

/// Run `<command> --version` and return the first non-empty output line. A
/// timeout, a non-zero exit and empty output are all errors with the command's
/// own words in them.
pub async fn probe_version(path: &Path) -> Result<String, String> {
    let mut command = tokio::process::Command::new(path);
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(VERSION_PROBE_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(format!("run {}: {error}", path.display())),
        Err(_) => {
            return Err(format!(
                "{} --version timed out after {}s",
                path.display(),
                VERSION_PROBE_TIMEOUT.as_secs()
            ))
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string();
    if !output.status.success() {
        let code = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string());
        return Err(format!(
            "{} --version exited with {code}: {}",
            path.display(),
            if line.is_empty() { "(no output)" } else { &line }
        ));
    }
    if line.is_empty() {
        return Err(format!(
            "{} --version printed no version",
            path.display()
        ));
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::catalog::harness_by_id;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "goble-harness-detect-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    fn executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_command_resolves_on_the_given_path_directories() {
        let dir = scratch("resolve");
        let script = executable(&dir, "fake-agent", "#!/bin/sh\nexit 0\n");
        let dirs = vec![dir.clone(), PathBuf::from("/definitely/not/here")];
        assert_eq!(resolve_command_in("fake-agent", &dirs), Some(script));
        assert_eq!(resolve_command_in("missing-agent", &dirs), None);
        // A path is checked directly, not searched for.
        assert_eq!(
            resolve_command_in("/bin/sh", &[]),
            Some(PathBuf::from("/bin/sh"))
        );
        assert_eq!(resolve_command_in("/bin/not-here", &[]), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_non_executable_file_does_not_resolve() {
        let dir = scratch("non-exec");
        let path = dir.join("not-a-program");
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        assert_eq!(resolve_command_in("not-a-program", &[dir]), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_configured_command_overrides_the_catalog() {
        let dir = scratch("override");
        let script = executable(&dir, "my-claude", "#!/bin/sh\nexit 0\n");
        let spec = harness_by_id("claude").expect("claude is in the catalog");
        let config = HarnessConfig {
            command: Some(script.display().to_string()),
            ..HarnessConfig::default()
        };
        assert_eq!(resolve_harness(spec, Some(&config)), Some(script.clone()));

        let status = status(spec, Some(&config));
        assert!(status.installed);
        assert!(status.configured);
        assert_eq!(status.command, script.display().to_string());
        assert_eq!(status.status_label(), "installed");
    }

    #[test]
    fn a_disabled_harness_reports_disabled() {
        let spec = harness_by_id("codex").expect("codex is in the catalog");
        let config = HarnessConfig {
            enabled: false,
            ..HarnessConfig::default()
        };
        let status = status(spec, Some(&config));
        assert!(!status.enabled);
        assert_eq!(status.status_label(), "disabled");
        assert!(status.configured);
    }

    #[test]
    fn the_catalog_id_is_the_config_key() {
        for spec in crate::harness::HARNESSES {
            assert_eq!(spec.config_key(), spec.id);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_version_probe_reads_the_first_output_line() {
        let dir = scratch("version");
        let script = executable(&dir, "versioned", "#!/bin/sh\necho 'fake-agent 1.2.3'\n");
        assert_eq!(probe_version(&script).await.unwrap(), "fake-agent 1.2.3");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failing_version_probe_reports_the_exit() {
        let dir = scratch("version-fail");
        let script = executable(
            &dir,
            "broken",
            "#!/bin/sh\necho 'no such flag' >&2\nexit 3\n",
        );
        let error = probe_version(&script).await.expect_err("non-zero exit");
        assert!(error.contains("exited with 3"), "{error}");
        assert!(error.contains("no such flag"), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn status_with_version_fills_in_the_version() {
        let dir = scratch("status-version");
        let script = executable(&dir, "codex", "#!/bin/sh\necho 'codex 0.9.1'\n");
        let spec = harness_by_id("codex").expect("codex is in the catalog");
        let config = HarnessConfig {
            command: Some(script.display().to_string()),
            ..HarnessConfig::default()
        };
        let status = status_with_version(spec, Some(&config)).await;
        assert!(status.installed);
        assert_eq!(status.version.as_deref(), Some("codex 0.9.1"));
        assert!(status.error.is_none());
    }

    #[test]
    fn a_missing_harness_reports_not_found_without_a_probe() {
        let spec = harness_by_id("claude").expect("claude is in the catalog");
        // An override path that does not exist: not installed, no version.
        let config = HarnessConfig {
            command: Some("/definitely/not/a/real/claude".to_string()),
            ..HarnessConfig::default()
        };
        let status = status(spec, Some(&config));
        assert!(!status.installed);
        assert_eq!(status.status_label(), "not found");
        assert!(status.path.is_none());
    }
}
