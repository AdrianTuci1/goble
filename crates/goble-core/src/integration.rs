//! T3 Code's integration seam, in Rust: one adapter per agent integration,
//! exposing detection, auth/connect state, models and effort levels to the
//! composer selector and Settings.
//!
//! The registry is deliberately data-driven: `AntigravityAdapter` reads the
//! real `agy` CLI (`--version`, `models`, config/auth), while the generic CLI
//! adapters only report PATH/version state. Nothing here invents model rows.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::harness::{path_dirs, resolve_command_in};

/// What an integration reports about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrationHealth {
    /// Installed, authenticated and able to run models.
    Connected,
    /// Installed but its sign-in state cannot be verified locally.
    Installed,
    /// Installed but signed out.
    NotAuthenticated,
    /// The CLI is not on PATH.
    NotFound,
    /// Disabled by the user's harness config.
    Disabled,
}

impl IntegrationHealth {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Installed => "installed · sign-in not checked",
            Self::NotAuthenticated => "needs auth",
            Self::NotFound => "not found",
            Self::Disabled => "disabled",
        }
    }
}

/// One model an integration can run, with the effort levels it supports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationModel {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
}

/// The snapshot both the selector and Settings render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationSnapshot {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub health: IntegrationHealth,
    pub auth: String,
    pub models: Vec<IntegrationModel>,
    pub config_path: Option<String>,
    pub detail: String,
}

impl IntegrationSnapshot {
    pub fn model_names(&self) -> Vec<String> {
        self.models.iter().map(|model| model.name.clone()).collect()
    }
}

/// Filesystem/process access, injectable so adapter tests use fixtures.
pub trait Probe {
    fn resolve(&self, command: &str) -> Option<PathBuf>;
    fn run(&self, program: &Path, args: &[&str]) -> Option<String>;
    fn exists(&self, path: &Path) -> bool;
    fn home(&self) -> Option<PathBuf>;
}

/// The real machine probe: PATH, `Command` and the filesystem.
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn resolve(&self, command: &str) -> Option<PathBuf> {
        resolve_command_in(command, &path_dirs())
    }

    fn run(&self, program: &Path, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// One integration's adapter. The registry is a `Vec<Box<dyn ...>>`, so adding
/// a provider is a new adapter, not a new branch in the composer.
pub trait IntegrationAdapter {
    fn id(&self) -> &'static str;
    fn snapshot(&self, probe: &dyn Probe) -> IntegrationSnapshot;
}

/// Antigravity's adapter: `agy` on PATH, its version, local config/auth state,
/// and the model list the CLI reports. Models are runtime data — on a machine
/// without `agy` the adapter reports `not found` and no models.
pub struct AntigravityAdapter;

impl IntegrationAdapter for AntigravityAdapter {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn snapshot(&self, probe: &dyn Probe) -> IntegrationSnapshot {
        let command = "agy";
        let path = probe.resolve(command);
        let config_candidates = config_candidates(probe);
        let config_path = config_candidates
            .iter()
            .find(|path| probe.exists(path))
            .cloned()
            .or_else(|| config_candidates.first().cloned());
        let version = path
            .as_ref()
            .and_then(|binary| probe.run(binary, &["--version"]))
            .map(|output| output.lines().next().unwrap_or("").trim().to_string())
            .filter(|line| !line.is_empty());
        let models = path
            .as_ref()
            .and_then(|binary| probe.run(binary, &["models"]))
            .map(|output| parse_model_lines(&output))
            .unwrap_or_default();
        let auth = auth_label(probe, config_path.as_deref(), &models);
        let health = match (&path, auth.as_str()) {
            (None, _) => IntegrationHealth::NotFound,
            (Some(_), "not authenticated") => IntegrationHealth::NotAuthenticated,
            (Some(_), _) if !models.is_empty() => IntegrationHealth::Connected,
            (Some(_), _) => IntegrationHealth::Installed,
        };
        let detail = match health {
            IntegrationHealth::NotFound => {
                "`agy` was not found on PATH. Install Antigravity and run `agy login`.".to_string()
            }
            IntegrationHealth::NotAuthenticated => {
                "`agy` is installed but not signed in. Run `agy login` to connect.".to_string()
            }
            IntegrationHealth::Installed => {
                "`agy` is installed. Sign-in is checked when a session starts.".to_string()
            }
            IntegrationHealth::Connected => {
                format!("`agy` is connected and reports {} model(s).", models.len())
            }
            IntegrationHealth::Disabled => {
                "Antigravity is disabled in the harness config.".to_string()
            }
        };
        IntegrationSnapshot {
            id: "antigravity".to_string(),
            name: "Antigravity".to_string(),
            icon: "harness-antigravity".to_string(),
            installed: path.is_some(),
            version,
            path: path.map(|path| path.to_string_lossy().to_string()),
            health,
            auth,
            models,
            config_path: config_path.map(|path| path.to_string_lossy().to_string()),
            detail,
        }
    }
}

fn config_candidates(probe: &dyn Probe) -> Vec<PathBuf> {
    let Some(home) = probe.home() else {
        return Vec::new();
    };
    vec![
        home.join(".config").join("antigravity"),
        home.join(".config").join("agy"),
        home.join(".antigravity"),
        home.join(".gemini").join("antigravity"),
    ]
}

fn auth_label(
    probe: &dyn Probe,
    config_path: Option<&Path>,
    models: &[IntegrationModel],
) -> String {
    if std::env::var_os("ANTIGRAVITY_API_KEY").is_some_and(|key| !key.is_empty()) {
        return "authenticated · API key".to_string();
    }
    if config_path.is_some_and(|path| probe.exists(path)) {
        // Antigravity's local check verifies installation, not Google sign-in;
        // T3 labels this "Sign-in not checked" rather than a false connected.
        return "sign-in not checked".to_string();
    }
    if !models.is_empty() {
        return "authenticated · CLI session".to_string();
    }
    "not authenticated".to_string()
}

/// Parse `agy models` (and ACP model-list fixtures): one model per non-empty
/// line, tolerating bullet/indent prefixes and an optional `id<TAB>name`.
pub fn parse_model_lines(output: &str) -> Vec<IntegrationModel> {
    let mut models = Vec::new();
    for raw in output.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '•']).trim();
        if line.is_empty()
            || line.ends_with(':')
            || line.to_ascii_lowercase().contains("available models")
        {
            continue;
        }
        let (id, name) = if let Some((id, name)) = line.split_once('\t') {
            (id.trim(), name.trim())
        } else if let Some((id, name)) = line.split_once(" - ") {
            (id.trim(), name.trim())
        } else {
            (line, line)
        };
        if id.is_empty() || models.iter().any(|model: &IntegrationModel| model.id == id) {
            continue;
        }
        models.push(IntegrationModel {
            id: id.to_string(),
            name: if name.is_empty() {
                id.to_string()
            } else {
                name.to_string()
            },
            efforts: Vec::new(),
        });
    }
    models
}

/// The default adapter set. Custom providers are already carried by the
/// configured model catalog; this registry is the CLI/integration seam.
pub struct IntegrationRegistry {
    adapters: Vec<Box<dyn IntegrationAdapter>>,
}

impl IntegrationRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self {
            adapters: Vec::new(),
        };
        registry.register(Box::new(AntigravityAdapter));
        for spec in crate::harness::HARNESSES {
            if spec.id == "goble" || spec.id == "antigravity" || spec.commands.is_empty() {
                continue;
            }
            registry.register(Box::new(CliAdapter::from_spec(spec)));
        }
        registry
    }

    pub fn register(&mut self, adapter: Box<dyn IntegrationAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn snapshot_all(&self) -> Vec<IntegrationSnapshot> {
        let probe = SystemProbe;
        self.adapters
            .iter()
            .map(|adapter| adapter.snapshot(&probe))
            .collect()
    }
}

/// A generic CLI integration with no runtime model catalog. It still reports
/// installed/version/path so Settings can show every integration honestly.
pub struct CliAdapter {
    id: &'static str,
    name: &'static str,
    command: &'static str,
}

impl CliAdapter {
    pub fn new(id: &'static str, name: &'static str, command: &'static str) -> Self {
        Self { id, name, command }
    }

    pub fn from_spec(spec: &'static crate::harness::HarnessSpec) -> Self {
        Self {
            id: spec.id,
            name: spec.name,
            command: spec.commands.first().copied().unwrap_or(spec.id),
        }
    }
}

impl IntegrationAdapter for CliAdapter {
    fn id(&self) -> &'static str {
        self.id
    }

    fn snapshot(&self, probe: &dyn Probe) -> IntegrationSnapshot {
        let path = probe.resolve(self.command);
        let version = path
            .as_ref()
            .and_then(|binary| probe.run(binary, &["--version"]))
            .map(|output| output.lines().next().unwrap_or("").trim().to_string())
            .filter(|line| !line.is_empty());
        // 463: read the CLI's runtime model list when it exposes one; otherwise
        // report no models instead of inventing any.
        let models = path
            .as_ref()
            .and_then(|binary| probe.run(binary, &["models"]))
            .map(|output| parse_model_lines(&output))
            .unwrap_or_default();
        IntegrationSnapshot {
            id: self.id.to_string(),
            name: self.name.to_string(),
            icon: format!("harness-{}", self.id),
            installed: path.is_some(),
            version,
            path: path.clone().map(|path| path.to_string_lossy().to_string()),
            health: if path.is_none() {
                IntegrationHealth::NotFound
            } else if models.is_empty() {
                IntegrationHealth::Installed
            } else {
                IntegrationHealth::Connected
            },
            auth: if path.is_some() {
                "CLI manages its own sign-in".to_string()
            } else {
                "not installed".to_string()
            },
            models,
            config_path: None,
            detail: if path.is_some() {
                format!("`{}` is on PATH.", self.command)
            } else {
                format!("`{}` was not found on PATH.", self.command)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FixtureProbe {
        commands: HashMap<String, PathBuf>,
        outputs: RefCell<HashMap<(String, String), String>>,
        existing: Vec<PathBuf>,
        home: Option<PathBuf>,
    }

    impl Probe for FixtureProbe {
        fn resolve(&self, command: &str) -> Option<PathBuf> {
            self.commands.get(command).cloned()
        }

        fn run(&self, program: &Path, args: &[&str]) -> Option<String> {
            let key = (program.to_string_lossy().to_string(), args.join(" "));
            self.outputs.borrow().get(&key).cloned()
        }

        fn exists(&self, path: &Path) -> bool {
            self.existing.iter().any(|candidate| candidate == path)
        }

        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
    }

    fn installed_probe(models: &str) -> FixtureProbe {
        let binary = PathBuf::from("/opt/antigravity/bin/agy");
        let mut probe = FixtureProbe {
            home: Some(PathBuf::from("/home/test")),
            ..Default::default()
        };
        probe.commands.insert("agy".to_string(), binary.clone());
        probe.outputs.borrow_mut().insert(
            (
                binary.to_string_lossy().to_string(),
                "--version".to_string(),
            ),
            "agy_acp_server_20250115_1\n".to_string(),
        );
        probe.outputs.borrow_mut().insert(
            (binary.to_string_lossy().to_string(), "models".to_string()),
            models.to_string(),
        );
        probe
    }

    #[test]
    fn no_agy_is_an_honest_not_found_snapshot() {
        let probe = FixtureProbe {
            home: Some(PathBuf::from("/home/test")),
            ..Default::default()
        };
        let snapshot = AntigravityAdapter.snapshot(&probe);
        assert_eq!(snapshot.health, IntegrationHealth::NotFound);
        assert!(!snapshot.installed);
        assert!(snapshot.models.is_empty());
        assert!(snapshot.detail.contains("not found on PATH"));
    }

    #[test]
    fn installed_without_sign_in_still_reports_its_models() {
        let probe = installed_probe(
            "Available models:\n  gemini-2.5-pro\tGemini 2.5 Pro\n  claude-sonnet-4\n",
        );
        let snapshot = AntigravityAdapter.snapshot(&probe);
        assert!(snapshot.installed);
        assert_eq!(snapshot.models.len(), 2);
        assert_eq!(snapshot.models[1].name, "claude-sonnet-4");
        assert_eq!(snapshot.health, IntegrationHealth::Connected);
    }

    #[test]
    fn a_config_directory_marks_sign_in_not_checked() {
        let mut probe = installed_probe("  gemini-2.5-pro\n");
        probe
            .existing
            .push(PathBuf::from("/home/test/.config/antigravity"));
        let snapshot = AntigravityAdapter.snapshot(&probe);
        assert_eq!(snapshot.auth, "sign-in not checked");
        assert_eq!(snapshot.health, IntegrationHealth::Connected);
        assert_eq!(
            snapshot.version.as_deref(),
            Some("agy_acp_server_20250115_1")
        );
    }

    #[test]
    fn the_registry_contains_every_t3_cli_adapter() {
        let registry = IntegrationRegistry::with_defaults();
        let ids: Vec<&str> = registry
            .adapters
            .iter()
            .map(|adapter| adapter.id())
            .collect();
        assert!(ids.contains(&"antigravity"));
        for expected in [
            "claude",
            "codex",
            "gemini",
            "copilot",
            "kiro",
            "opencode",
            "omp",
            "pi",
            "hermes",
            "minimax",
            "minimax-code",
        ] {
            assert!(ids.contains(&expected), "missing {expected}: {ids:?}");
        }
        assert!(ids.len() >= 20, "the registry carries the full T3 CLI set");
    }
}

/// 499: the persisted/re-detected state machine every integration follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationAuthState {
    NotInstalled,
    Installed,
    NeedsAuth,
    Authenticating,
    Connected,
    Error,
}

impl IntegrationAuthState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "not installed",
            Self::Installed => "installed",
            Self::NeedsAuth => "needs auth",
            Self::Authenticating => "authenticating",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }
}

/// A tiny deterministic state machine used by Settings' auth action and the
/// selector's NeedsAuth gate. It never reports Connected unless the adapter
/// actually reports models after a successful check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthStateMachine {
    pub state: IntegrationAuthState,
    pub error: Option<String>,
}

impl AuthStateMachine {
    pub fn from_snapshot(snapshot: &IntegrationSnapshot) -> Self {
        let state = match snapshot.health {
            IntegrationHealth::NotFound => IntegrationAuthState::NotInstalled,
            IntegrationHealth::Disabled => IntegrationAuthState::NotInstalled,
            IntegrationHealth::NotAuthenticated => IntegrationAuthState::NeedsAuth,
            IntegrationHealth::Installed => IntegrationAuthState::Installed,
            IntegrationHealth::Connected => IntegrationAuthState::Connected,
        };
        Self { state, error: None }
    }

    pub fn begin_auth(&mut self) -> bool {
        if self.state == IntegrationAuthState::NotInstalled {
            return false;
        }
        self.state = IntegrationAuthState::Authenticating;
        self.error = None;
        true
    }

    pub fn succeed(&mut self, has_models: bool) {
        self.state = if has_models {
            IntegrationAuthState::Connected
        } else {
            IntegrationAuthState::Installed
        };
        self.error = None;
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.state = IntegrationAuthState::Error;
        self.error = Some(error.into());
    }

    pub fn cancel(&mut self, fallback: IntegrationAuthState) {
        if self.state == IntegrationAuthState::Authenticating {
            self.state = fallback;
        }
    }
}

#[cfg(test)]
mod auth_state_tests {
    use super::*;

    fn snapshot(health: IntegrationHealth) -> IntegrationSnapshot {
        IntegrationSnapshot {
            id: "x".to_string(),
            name: "X".to_string(),
            icon: "plug".to_string(),
            installed: health != IntegrationHealth::NotFound,
            version: None,
            path: None,
            health,
            auth: String::new(),
            models: Vec::new(),
            config_path: None,
            detail: String::new(),
        }
    }

    #[test]
    fn auth_moves_only_through_the_documented_states() {
        let mut machine = AuthStateMachine::from_snapshot(&snapshot(IntegrationHealth::Installed));
        assert_eq!(machine.state, IntegrationAuthState::Installed);
        assert!(machine.begin_auth());
        assert_eq!(machine.state, IntegrationAuthState::Authenticating);
        machine.succeed(true);
        assert_eq!(machine.state, IntegrationAuthState::Connected);

        let mut missing = AuthStateMachine::from_snapshot(&snapshot(IntegrationHealth::NotFound));
        assert!(!missing.begin_auth(), "a missing CLI cannot authenticate");
        assert_eq!(missing.state, IntegrationAuthState::NotInstalled);
    }

    #[test]
    fn auth_failure_and_cancel_are_explicit() {
        let mut machine = AuthStateMachine::from_snapshot(&snapshot(IntegrationHealth::Installed));
        machine.begin_auth();
        machine.fail("browser closed");
        assert_eq!(machine.state, IntegrationAuthState::Error);
        assert_eq!(machine.error.as_deref(), Some("browser closed"));
        machine.cancel(IntegrationAuthState::Installed);
        assert_eq!(machine.state, IntegrationAuthState::Error);
    }

    #[test]
    fn success_without_models_never_claims_connected() {
        let mut machine = AuthStateMachine::from_snapshot(&snapshot(IntegrationHealth::Installed));
        machine.begin_auth();
        machine.succeed(false);
        assert_eq!(machine.state, IntegrationAuthState::Installed);
    }
}
