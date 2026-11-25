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
            Self::NotAuthenticated => "not authenticated",
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
        registry.register(Box::new(CliAdapter::new("claude", "Claude Code", "claude")));
        registry.register(Box::new(CliAdapter::new("codex", "Codex", "codex")));
        registry.register(Box::new(CliAdapter::new("gemini", "Gemini", "gemini")));
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
        IntegrationSnapshot {
            id: self.id.to_string(),
            name: self.name.to_string(),
            icon: format!("harness-{}", self.id),
            installed: path.is_some(),
            version,
            path: path.clone().map(|path| path.to_string_lossy().to_string()),
            health: if self.id == "antigravity" {
                IntegrationHealth::Installed
            } else if path.is_some() {
                IntegrationHealth::Installed
            } else {
                IntegrationHealth::NotFound
            },
            auth: if path.is_some() {
                "CLI manages its own sign-in".to_string()
            } else {
                "not installed".to_string()
            },
            models: Vec::new(),
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
        assert_eq!(ids, vec!["antigravity", "claude", "codex", "gemini"]);
    }
}
