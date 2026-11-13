//! What a harness advertises to a composer, and how the host groups it.
//!
//! The reference client reads its command list, provider models and the
//! thinking levels those models accept from the agent session itself, so the
//! composer lists data instead of a table written into the UI. Goble composes
//! the same answer for the native harness here, from the configuration and the
//! model catalogue the turns already run on, and hands it to the app in the
//! [`HarnessCatalog`] shape every harness reports through
//! [`goble_harness_runtime::HarnessRuntime::catalog`].
//!
//! The app also draws a model tray with one column per harness; [`ModelGroup`]
//! is that column's data, grouped by the provider each declared model resolves
//! to and recognised through the same [`goble_core::harness::harness_for`] map
//! the sidebar and the pane bar use.

use std::path::Path;

use goble_harness_types::{HarnessCatalog, HarnessCommand, HarnessModel};

use super::DesktopState;

/// The thinking levels the native harness runs a model at: its reasoning loop
/// either plans the turn in explicit steps first, or answers it directly.
fn native_thinking() -> Vec<String> {
    vec!["off".to_string(), "on".to_string()]
}

/// The commands the native harness answers, in the order a composer lists them.
///
/// Each carries the id the app runs it by; the names are the harness's, not the
/// UI's, so a second harness that reports its own list shows its own commands
/// without the app learning anything about it.
pub fn harness_commands() -> Vec<HarnessCommand> {
    vec![
        HarnessCommand::slash("new", "Start a fresh conversation", "new"),
        HarnessCommand::slash("clear", "Clear this conversation's transcript", "clear"),
        HarnessCommand::slash("fork", "Branch this conversation into a new one", "fork"),
        HarnessCommand::slash("stop", "Stop the running turn", "stop"),
        HarnessCommand::slash("shortcuts", "Show every keyboard shortcut", "shortcuts"),
        HarnessCommand::slash("settings", "Open settings", "settings"),
        HarnessCommand::slash("space", "Open a new space", "space"),
        HarnessCommand::slash("split", "Split this pane to the right", "split"),
        HarnessCommand::slash("terminal", "Open a new terminal pane", "terminal"),
        HarnessCommand::slash("close", "Close this pane", "close"),
        HarnessCommand::slash("sidebar", "Toggle the right sidebar", "sidebar"),
        HarnessCommand::slash("fullscreen", "Toggle full screen", "fullscreen"),
        HarnessCommand::slash("maximize", "Expand this pane over the panes space", "maximize"),
        HarnessCommand::slash("projects", "Open the project list", "projects"),
        HarnessCommand::slash("scheduled", "Open the scheduled tasks", "scheduled"),
        HarnessCommand::slash("tasks", "Open the workflow runs in this pane", "tasks"),
        HarnessCommand::slash("workflows", "Open the workflow catalog", "workflows"),
        HarnessCommand::slash("mcps", "Open the MCP servers", "mcps"),
        HarnessCommand::slash("theme", "Switch between the dark and light theme", "theme"),
        HarnessCommand::slash("vim-mode", "Toggle vim-style editing", "vim-mode"),
        HarnessCommand::slash(
            "always-approve",
            "Stop asking before running commands",
            "always-approve",
        ),
    ]
}

/// The skills the harness can load, read from every `<dir>/SKILL.md` under the
/// home's `skills/` directory.
///
/// A skill is named by its front matter's `name` (the directory's name when the
/// file states none) and described by its `description`. Discovery is tolerant:
/// a directory without a readable `SKILL.md` offers nothing, so a half-written
/// skill never breaks the menu.
pub fn discover_skills(root: &Path) -> Vec<HarnessCommand> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some((name, description)) = read_skill(&path) else {
            continue;
        };
        skills.push(HarnessCommand::skill(name, description));
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// A skill's name and description, from its `SKILL.md` front matter. `None`
/// when the directory holds no readable file.
fn read_skill(dir: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let fallback_name = dir.file_name()?.to_string_lossy().to_string();
    let mut name = String::new();
    let mut description = String::new();
    for line in text.lines().take(20) {
        let line = line.trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        match key.trim() {
            "name" if name.is_empty() => name = value,
            "description" if description.is_empty() => description = value,
            _ => {}
        }
    }
    if name.trim().is_empty() {
        name = fallback_name;
    }
    if description.trim().is_empty() {
        // The first prose line under the front matter is the description the
        // file never labelled.
        description = text
            .lines()
            .map(str::trim)
            .find(|line| {
                !line.is_empty()
                    && !line.starts_with('#')
                    && !line.starts_with("---")
                    && !line.contains(": ")
            })
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect();
    }
    Some((name, description))
}

/// A provider's name as the picker shows it.
pub fn provider_label(provider: &str) -> String {
    match provider.to_ascii_lowercase().as_str() {
        "openai" => "OpenAI".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "xai" | "x-ai" => "xAI".to_string(),
        "google" | "gemini" => "Google".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "ollama" => "Ollama".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "mock" => "Mock".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Custom".to_string(),
            }
        }
    }
}

/// The provider a `[model.<slug>]` entry talks to, read from its `base_url`: the
/// endpoint is what names the family, so two entries pointing at the same host
/// land in the same tray column. `None` when the URL is unset or unknown — the
/// caller falls back to the provider in force.
fn provider_for_base_url(url: &str) -> Option<&'static str> {
    let url = url.to_ascii_lowercase();
    if url.is_empty() {
        return None;
    }
    if url.contains("anthropic") {
        Some("anthropic")
    } else if url.contains("deepseek") {
        Some("deepseek")
    } else if url.contains("x.ai") {
        Some("xai")
    } else if url.contains("googleapis") || url.contains("generativelanguage") {
        Some("google")
    } else if url.contains("openrouter") {
        Some("openrouter")
    } else if url.contains("11434") || url.contains("ollama") {
        Some("ollama")
    } else if url.contains("openai") {
        Some("openai")
    } else {
        None
    }
}

/// One column-one row of the composer's model tray: a harness mark, the label
/// beside it, and the models that harness runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelGroup {
    /// The harness id the row draws its mark from (`goble`, `claude`, `grok`);
    /// the provider's own name when no catalog entry recognises it.
    pub harness: String,
    /// The name the row shows.
    pub label: String,
    pub models: Vec<HarnessModel>,
}

impl DesktopState {
    /// What the native harness advertises to a composer: its commands, the
    /// skills the home holds, and the models `provider` offers with the
    /// thinking levels each runs at.
    pub fn composer_catalog(&self, provider: &str) -> HarnessCatalog {
        let mut commands = harness_commands();
        // The `/model <name>` entries are one per model, so they are written
        // here where the models are known rather than in the fixed table.
        for model in self.available_models(provider) {
            commands.push(HarnessCommand::slash(
                format!("model {model}"),
                "Run this pane on another model",
                format!("model:{model}"),
            ));
        }
        if let Some(home) = goble_core::app_home::GobleHome::locate().ok() {
            commands.extend(discover_skills(&home.root().join("skills")));
        }
        HarnessCatalog::empty()
            .with_commands(commands)
            .with_models(self.model_models(provider))
    }

    /// The models `provider` offers, each with the thinking levels the native
    /// harness runs it at.
    fn model_models(&self, provider: &str) -> Vec<HarnessModel> {
        let config = self.config();
        self.available_models(provider)
            .into_iter()
            .map(|model| {
                let name = config
                    .model_for(&model)
                    .and_then(|(slug, entry)| entry.name.clone().or_else(|| Some(slug.to_string())))
                    .unwrap_or_else(|| model.clone());
                HarnessModel::new(model, name).with_thinking(native_thinking())
            })
            .collect()
    }

    /// The model tray's first column: every provider the configuration declares
    /// models for, each group holding the models that endpoint runs. A config
    /// that declares nothing falls back to the provider in force, so the tray
    /// always has the models the composer can actually apply.
    pub fn model_groups(&self, provider: &str) -> Vec<ModelGroup> {
        let config = self.config();
        let mut groups: Vec<ModelGroup> = Vec::new();
        let mut push = |provider: &str, model: HarnessModel| {
            let harness = goble_core::harness::harness_for(provider)
                .map(|spec| spec.id.to_string())
                .unwrap_or_else(|| provider.to_string());
            let label = provider_label(provider);
            match groups
                .iter_mut()
                .find(|group| group.harness == harness && group.label == label)
            {
                Some(group) => group.models.push(model),
                None => groups.push(ModelGroup {
                    harness,
                    label,
                    models: vec![model],
                }),
            }
        };
        let declared = config.model_ids();
        if declared.is_empty() {
            for model in self.model_models(provider) {
                push(provider, model);
            }
            return groups;
        }
        for model in declared {
            let owner = config
                .model_for(&model)
                .and_then(|(_, entry)| entry.base_url.as_deref())
                .and_then(provider_for_base_url)
                .unwrap_or(provider)
                .to_string();
            let name = config
                .model_for(&model)
                .and_then(|(slug, entry)| entry.name.clone().or_else(|| Some(slug.to_string())))
                .unwrap_or_else(|| model.clone());
            push(
                &owner,
                HarnessModel::new(model, name).with_thinking(native_thinking()),
            );
        }
        groups
    }
}
