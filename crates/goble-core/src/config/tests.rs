//! The config schema, the round-trip and the migration off the pre-grok shape.

use super::*;

/// The shape `~/.grok/config.toml` is written in.
const GROK_SHAPED: &str = r##"
[cli]
installer = "internal"

[marketplace]
official_marketplace_auto_installed = true

[[marketplace.sources]]
name = "xAI Official"
git = "https://github.com/xai-org/plugin-marketplace.git"

[model.deepseek]
model = "deepseek-flash"
base_url = "https://api.deepseek.com"
name = "Deepseek-V4-Flash"
api_key = "sk-deepseek"
max_completion_tokens = 16384
context_window = 256000

[model.dspro]
model = "deepseek-v4-pro"
base_url = "https://api.deepseek.com"
name = "Deepseek-V4-Pro"
api_key = "sk-deepseek"
max_completion_tokens = 8192
context_window = 256000

[model.kimi]
model = "kimi-k2.7"
base_url = "https://api.kimi.com/coding/v1"
name = "Kimi K2.7"
api_key = "sk-kimi"
max_completion_tokens = 8192
context_window = 256000

[model.qwen]
model = "qwen3.8-flash"
base_url = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
name = "Qwen 3.8 Flash"
api_key = "sk-qwen"
max_completion_tokens = 8192
context_window = 256000

[models]
default = "deepseek"

[ui]
max_thoughts_width = 120
compact_mode = false
show_timeline = false

[privacy]
privacy_banner_acked = "2026-08-24T19:13:05Z"

[mcp_servers.penpot]
url = "https://design.penpot.app/mcp/stream"
enabled = true
"##;

/// The config `~/.goble/config.toml` held before the schema change.
const LEGACY_GOBLE: &str = r##"
version = 1

[llm]
default_provider = "openai"
default_model = "gpt-4o"
providers = []

[[llm.models]]
id = "gpt-4o"
provider = "openai"
label = "gpt-4o"
api_key_secret_id = "sk-test"
enabled = true

[theme]
dark = true
accent = "#14b8a6"
"##;

fn grok_shaped() -> GobleConfig {
    GobleConfig::from_toml(GROK_SHAPED).expect("a grok config parses")
}

#[test]
fn grok_shaped_document_parses_into_the_model_map() {
    let config = grok_shaped();

    assert_eq!(config.model.len(), 4);
    let deepseek = &config.model["deepseek"];
    assert_eq!(deepseek.model, "deepseek-flash");
    assert_eq!(deepseek.name.as_deref(), Some("Deepseek-V4-Flash"));
    assert_eq!(
        deepseek.base_url.as_deref(),
        Some("https://api.deepseek.com")
    );
    assert_eq!(deepseek.resolved_key(), Some("sk-deepseek"));
    assert_eq!(deepseek.max_completion_tokens, Some(16384));
    assert_eq!(deepseek.context_window, Some(256000));
    assert_eq!(
        config.model["qwen"].base_url.as_deref(),
        Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1")
    );
    assert_eq!(config.models.default.as_deref(), Some("deepseek"));
    // A `[mcp_servers.<name>]` entry is an HTTP endpoint with `enabled`.
    let penpot = &config.mcp_servers["penpot"];
    assert_eq!(
        penpot.url.as_deref(),
        Some("https://design.penpot.app/mcp/stream")
    );
    assert!(penpot.enabled);
    // Nothing in a grok file is required: no `version`, no `[llm]`.
    assert!(config.llm.is_empty());
}

#[test]
fn to_toml_emits_the_grok_layout() {
    let toml = grok_shaped().to_toml().expect("serialize");

    assert!(toml.contains("[model.deepseek]"), "{toml}");
    assert!(toml.contains(r#"model = "deepseek-flash""#), "{toml}");
    assert!(toml.contains("[models]"), "{toml}");
    assert!(toml.contains(r#"default = "deepseek""#), "{toml}");
    assert!(toml.contains("[mcp_servers.penpot]"), "{toml}");
    assert!(!toml.contains("[llm]"), "{toml}");
    assert!(!toml.contains("version ="), "{toml}");
}

/// The header is the file's own documentation, and every write carries it: a
/// save that rewrote only the body left the user with a config file that says
/// nothing about what may go in it.
#[test]
fn every_write_opens_with_the_header() {
    let toml = GobleConfig::default().to_toml().expect("serialize");
    assert!(toml.starts_with(CONFIG_HEADER), "{toml}");

    // A save after a model and a theme change — the write the settings make —
    // carries it too, and the file still parses to what was written.
    let mut config = GobleConfig::default();
    config
        .model
        .entry("deepseek".to_string())
        .or_default()
        .api_key = Some("sk-test".to_string());
    config.models.default = Some("deepseek".to_string());
    config.theme.dark = false;

    let toml = config.to_toml().expect("serialize");
    assert!(toml.starts_with(CONFIG_HEADER), "{toml}");
    assert_eq!(GobleConfig::from_toml(&toml).expect("parse"), config);
}

/// What the header documents is what the file holds: the three sections, the
/// shape of a model entry, and the promise that a section goble does not model
/// survives.
#[test]
fn the_header_documents_the_file() {
    assert!(CONFIG_HEADER.starts_with("# goble configuration"));
    assert!(CONFIG_HEADER.contains("[model.deepseek]"));
    assert!(CONFIG_HEADER.contains(r#"model = "deepseek-flash""#));
    assert!(CONFIG_HEADER.contains(r#"api_key = "sk-...""#));
    assert!(CONFIG_HEADER.contains(r#"default = "deepseek""#));
    assert!(CONFIG_HEADER.contains("[mcp_servers.penpot]"));
    assert!(CONFIG_HEADER.contains("[remote_agents.builder]"));
    assert!(CONFIG_HEADER.contains("[remote_agents.reviewer]"));
    assert!(CONFIG_HEADER.contains(r#"provider = "modal""#));
    assert!(CONFIG_HEADER.contains(r#"provider = "e2b""#));
    assert!(CONFIG_HEADER.contains("[theme]"));
    assert!(CONFIG_HEADER.contains("kept exactly as they were written"));
    // The lines a reader follows are comments, so the file they open parses.
    let (parsed, problems) = GobleConfig::load_toml(CONFIG_HEADER).expect("the header alone is TOML");
    assert_eq!(parsed, GobleConfig::default());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_grok_file_round_trips_with_its_unmodelled_sections() {
    let config = grok_shaped();
    let toml = config.to_toml().expect("serialize");
    let reparsed = GobleConfig::from_toml(&toml).expect("the emitted file parses");

    assert_eq!(reparsed, config);
    // Sections goble does not model (grok's `[cli]`, `[ui]`, `[marketplace]`,
    // `[privacy]`) survive a save rather than being dropped.
    for section in ["[cli]", "[ui]", "[privacy]", "[marketplace]"] {
        assert!(toml.contains(section), "{section} missing from:\n{toml}");
    }
    assert!(toml.contains("[[marketplace.sources]]"), "{toml}");
}

#[test]
fn partial_file_with_only_the_default_parses() {
    let config = GobleConfig::from_toml("[models]\ndefault = \"deepseek\"\n").expect("parse");

    assert_eq!(config.models.default.as_deref(), Some("deepseek"));
    assert!(config.model.is_empty());
    assert_eq!(config.theme, ThemeConfig::default());
}

#[test]
fn an_empty_file_is_the_default_config() {
    assert_eq!(
        GobleConfig::from_toml("").expect("parse"),
        GobleConfig::default()
    );
}

#[test]
fn default_model_resolves_to_the_model_id() {
    let config = GobleConfig::from_toml(
        r#"
[model.dspro]
model = "deepseek-v4-pro"
name = "Deepseek-V4-Pro"

[models]
default = "dspro"
"#,
    )
    .expect("parse");

    assert_eq!(
        config.default_model_id().as_deref(),
        Some("deepseek-v4-pro"),
        "the default slug resolves to the id sent to the API"
    );
    assert_eq!(config.model_ids(), vec!["deepseek-v4-pro".to_string()]);
}

#[test]
fn a_default_naming_no_table_resolves_to_itself() {
    let config = GobleConfig::from_toml("[models]\ndefault = \"grok-4.6\"\n").expect("parse");

    assert_eq!(config.default_model_id().as_deref(), Some("grok-4.6"));
}

#[test]
fn hidden_models_stay_out_of_the_picker() {
    let config = GobleConfig::from_toml(
        r#"
[model.kimi]
model = "kimi-k2.7"
hidden = true

[model.qwen]
model = "qwen3.8-flash"
"#,
    )
    .expect("parse");

    assert_eq!(config.model_ids(), vec!["qwen3.8-flash".to_string()]);
    assert!(config.model_for("kimi").is_some());
}

// ---- The migration off the pre-grok shape ------------------------------

#[test]
fn the_old_goble_file_loads_with_its_models_and_theme_intact() {
    let (config, problems) = GobleConfig::load_toml(LEGACY_GOBLE).expect("valid TOML");

    assert!(problems.is_empty(), "{problems:?}");
    let gpt = &config.model["gpt-4o"];
    assert_eq!(gpt.model, "gpt-4o");
    assert_eq!(gpt.name.as_deref(), Some("gpt-4o"));
    // `api_key_secret_id` was never resolved through the vault by any reader,
    // so the key it held moves into the inline `api_key`.
    assert_eq!(gpt.resolved_key(), Some("sk-test"));
    assert_ne!(gpt.hidden, Some(true));
    assert_eq!(config.models.default.as_deref(), Some("gpt-4o"));
    assert_eq!(config.default_model_id().as_deref(), Some("gpt-4o"));
    assert!(config.theme.dark);
    assert_eq!(config.theme.accent, "#14b8a6");
}

#[test]
fn the_next_save_writes_the_migrated_shape() {
    let (config, _) = GobleConfig::load_toml(LEGACY_GOBLE).expect("valid TOML");
    let toml = config.to_toml().expect("serialize");

    assert!(toml.contains("[model.gpt-4o]"), "{toml}");
    assert!(toml.contains(r#"api_key = "sk-test""#), "{toml}");
    assert!(toml.contains("[models]"), "{toml}");
    assert!(toml.contains("[theme]"), "{toml}");
    assert!(!toml.contains("[llm]"), "{toml}");
    assert!(!toml.contains("api_key_secret_id"), "{toml}");

    let reparsed = GobleConfig::from_toml(&toml).expect("the migrated file parses");
    assert_eq!(reparsed.model, config.model);
    assert_eq!(reparsed.models, config.models);
    assert_eq!(reparsed.theme, config.theme);
}

#[test]
fn migration_is_idempotent() {
    let (mut config, _) = GobleConfig::load_toml(LEGACY_GOBLE).expect("valid TOML");
    let once = config.clone();
    config.absorb_legacy();
    assert_eq!(config, once);
}

#[test]
fn a_disabled_legacy_model_migrates_to_hidden() {
    let (config, _) = GobleConfig::load_toml(
        r#"
[[llm.models]]
id = "gpt-4o"
provider = "openai"
enabled = false
"#,
    )
    .expect("valid TOML");

    assert_eq!(config.model["gpt-4o"].hidden, Some(true));
    assert!(config.model_ids().is_empty());
}

#[test]
fn a_legacy_provider_becomes_a_model_entry() {
    let (config, _) = GobleConfig::load_toml(
        r#"
[llm]
default_provider = "deepseek"
default_model = "deepseek-chat"

[[llm.providers]]
name = "deepseek"
api_key_secret_id = "sk-live"
base_url = "https://api.deepseek.com/v1"
model = "deepseek-chat"
"#,
    )
    .expect("valid TOML");

    assert_eq!(config.model["deepseek"].model, "deepseek-chat");
    assert_eq!(config.model["deepseek"].resolved_key(), Some("sk-live"));
    assert_eq!(config.models.default.as_deref(), Some("deepseek"));
}

#[test]
fn migrated_slugs_stay_unique_and_round_trip() {
    let (config, _) = GobleConfig::load_toml(
        r#"
[[llm.models]]
id = "claude-3.5-sonnet"
provider = "anthropic"
label = "Sonnet"

[[llm.models]]
id = "anthropic/claude-3.5-sonnet"
provider = "openrouter"
label = "Sonnet via OpenRouter"
"#,
    )
    .expect("valid TOML");

    assert_eq!(config.model.len(), 2, "{:?}", config.model.keys());

    let toml = config.to_toml().expect("serialize");
    let reparsed = GobleConfig::from_toml(&toml).expect("a quoted slug key parses");
    assert_eq!(reparsed.model, config.model);
}

// ---- A file that does not parse ---------------------------------------

#[test]
fn a_bad_section_costs_its_section_and_not_the_file() {
    let (config, problems) = GobleConfig::load_toml(
        r#"
[theme]
dark = "yes"

[model.gpt-4o]
model = "gpt-4o"
api_key = "sk-test"

[models]
default = "gpt-4o"
"#,
    )
    .expect("this is valid TOML");

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("[theme]"), "{problems:?}");
    // The theme falls back to its default; the models are kept.
    assert_eq!(config.theme, ThemeConfig::default());
    assert_eq!(config.model["gpt-4o"].resolved_key(), Some("sk-test"));
    assert_eq!(config.default_model_id().as_deref(), Some("gpt-4o"));
}

#[test]
fn text_that_is_not_toml_is_a_hard_error() {
    assert!(GobleConfig::load_toml("this is not = = toml").is_err());
    assert!(GobleConfig::from_toml("[model.gpt-4o]\nmodel = 4").is_err());
}

#[test]
fn an_unmodelled_top_level_scalar_is_not_kept() {
    // The old `version = 1` must not come back after the tables, where TOML
    // cannot represent it.
    let config =
        GobleConfig::from_toml("version = 1\n[models]\ndefault = \"gpt-4o\"\n").expect("parse");
    assert!(config.extra.is_empty());

    let toml = config.to_toml().expect("serialize");
    assert!(!toml.contains("version"), "{toml}");
    assert_eq!(
        GobleConfig::from_toml(&toml)
            .expect("parse")
            .models
            .default
            .as_deref(),
        Some("gpt-4o")
    );
}

#[test]
fn the_seeded_default_is_the_new_shape() {
    let toml = GobleConfig::default().to_toml().expect("serialize");

    assert!(toml.contains("[theme]"), "{toml}");
    assert!(toml.contains(r##"accent = "#14b8a6""##), "{toml}");
    assert!(!toml.contains("[llm]"), "{toml}");
    assert!(!toml.contains("version"), "{toml}");
    assert_eq!(
        GobleConfig::from_toml(&toml).expect("parse"),
        GobleConfig::default()
    );
}

#[test]
fn an_mcp_stdio_entry_parses_and_is_enabled_by_default() {
    let config = GobleConfig::from_toml(
        "[mcp_servers.gh]\ncommand = \"npx\"\nargs = [\"-y\", \"gh\"]\n",
    )
    .expect("parse");

    let server = &config.mcp_servers["gh"];
    assert_eq!(server.command.as_deref(), Some("npx"));
    assert_eq!(server.args, vec!["-y".to_string(), "gh".to_string()]);
    assert!(server.enabled);
    assert!(server.url.is_none());
}

// ---- Remote agents ------------------------------------------------------

/// The phase-one shape: a name per sandbox agent, a provider, and the
/// credentials that provider reads.
const REMOTE_AGENTS: &str = r#"
[remote_agents.builder]
provider = "modal"
token_id = "ak-modal"
token_secret = "as-modal"
image = "ubuntu:24.04"
environment = "dev"

[remote_agents.reviewer]
provider = "e2b"
api_key = "e2b-key"
region = "us-east-1"
enabled = false
"#;

/// Parse one agent table on its own, for the credential cases.
fn one_agent(body: &str) -> RemoteAgentConfig {
    let toml = format!("[remote_agents.test]\n{body}");
    GobleConfig::from_toml(&toml)
        .expect("the table parses")
        .remote_agents
        .remove("test")
        .expect("the table is there")
}

#[test]
fn remote_agents_parse_and_round_trip() {
    let config = GobleConfig::from_toml(REMOTE_AGENTS).expect("parse");
    assert_eq!(config.remote_agents.len(), 2);

    let builder = &config.remote_agents["builder"];
    assert_eq!(builder.provider, RemoteAgentProvider::Modal);
    assert_eq!(builder.provider.as_str(), "modal");
    assert_eq!(builder.token_id.as_deref(), Some("ak-modal"));
    assert_eq!(builder.token_secret.as_deref(), Some("as-modal"));
    assert_eq!(builder.image.as_deref(), Some("ubuntu:24.04"));
    assert_eq!(builder.environment.as_deref(), Some("dev"));
    assert!(builder.enabled);
    assert!(builder.missing_credentials().is_empty());
    assert_eq!(builder.status_label(), "ready");

    let reviewer = &config.remote_agents["reviewer"];
    assert_eq!(reviewer.provider, RemoteAgentProvider::E2b);
    assert_eq!(reviewer.api_key.as_deref(), Some("e2b-key"));
    assert_eq!(reviewer.region.as_deref(), Some("us-east-1"));
    assert!(!reviewer.enabled);
    assert_eq!(reviewer.status_label(), "disabled");

    let toml = config.to_toml().expect("serialize");
    assert!(toml.contains("[remote_agents.builder]"), "{toml}");
    assert!(toml.contains(r#"provider = "modal""#), "{toml}");
    assert!(toml.contains("[remote_agents.reviewer]"), "{toml}");
    assert!(toml.contains(r#"provider = "e2b""#), "{toml}");
    let reparsed = GobleConfig::from_toml(&toml).expect("the emitted file parses");
    assert_eq!(reparsed, config);
}

#[test]
fn an_empty_remote_agent_map_is_not_written() {
    let toml = GobleConfig::default().to_toml().expect("serialize");
    let body = toml.strip_prefix(CONFIG_HEADER).expect("the header leads");
    assert!(!body.contains("[remote_agents"), "{body}");
}

#[test]
fn a_partial_remote_agent_table_keeps_its_defaults() {
    let agent = one_agent(r#"provider = "ssh""#);

    assert_eq!(agent.provider, RemoteAgentProvider::Ssh);
    assert!(agent.enabled, "enabled defaults to true");
    assert!(agent.api_key.is_none());
    assert!(agent.api_url.is_none());
    assert_eq!(agent.status_label(), "ready", "ssh needs no credentials");
}

#[test]
fn an_unknown_remote_agent_provider_costs_only_its_own_table() {
    let (config, problems) = GobleConfig::load_toml(
        r#"
[theme]
dark = false

[remote_agents.good]
provider = "docker"

[remote_agents.bad]
provider = "aws"
"#,
    )
    .expect("this is valid TOML");

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("[remote_agents.bad]"),
        "the problem names the table: {problems:?}"
    );
    assert!(
        problems[0].contains("unknown variant"),
        "the problem says what was wrong: {problems:?}"
    );
    assert!(
        config.remote_agents.contains_key("good"),
        "the other agent is kept"
    );
    assert!(!config.remote_agents.contains_key("bad"));
    assert!(!config.theme.dark, "the rest of the file is kept");
}

#[test]
fn a_remote_agent_table_without_a_provider_is_reported() {
    let (config, problems) = GobleConfig::load_toml("[remote_agents.nameless]\nenabled = true\n")
        .expect("this is valid TOML");

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("[remote_agents.nameless]"),
        "{problems:?}"
    );
    assert!(
        problems[0].contains("missing field `provider`"),
        "{problems:?}"
    );
    assert!(config.remote_agents.is_empty());
}

#[test]
fn remote_agent_missing_credentials_names_the_keys() {
    let modal = one_agent(r#"provider = "modal""#);
    assert_eq!(
        modal.missing_credentials(),
        vec!["MODAL_TOKEN_ID", "MODAL_TOKEN_SECRET"]
    );
    assert_eq!(
        modal.status_label(),
        "missing MODAL_TOKEN_ID, MODAL_TOKEN_SECRET"
    );

    // One half set: only the other half is reported.
    let half = one_agent("provider = \"modal\"\ntoken_id = \"ak-modal\"\n");
    assert_eq!(half.missing_credentials(), vec!["MODAL_TOKEN_SECRET"]);

    // A blank value is not a credential.
    let blank = one_agent("provider = \"modal\"\ntoken_id = \"ak-modal\"\ntoken_secret = \"  \"\n");
    assert_eq!(blank.missing_credentials(), vec!["MODAL_TOKEN_SECRET"]);

    let e2b = one_agent(r#"provider = "e2b""#);
    assert_eq!(e2b.missing_credentials(), vec!["E2B_API_KEY"]);
    assert_eq!(e2b.status_label(), "missing E2B_API_KEY");

    let daytona = one_agent(r#"provider = "daytona""#);
    assert_eq!(daytona.missing_credentials(), vec!["DAYTONA_API_KEY"]);

    let box_agent = one_agent(r#"provider = "box""#);
    assert_eq!(box_agent.missing_credentials(), vec!["BOX_API_KEY"]);

    // These two read no credentials at all.
    assert!(one_agent(r#"provider = "docker""#)
        .missing_credentials()
        .is_empty());
    assert!(one_agent(r#"provider = "ssh""#)
        .missing_credentials()
        .is_empty());

    // The credential, once present, clears the report.
    let ready = one_agent("provider = \"e2b\"\napi_key = \"sk-e2b\"\n");
    assert!(ready.missing_credentials().is_empty());
    assert_eq!(ready.status_label(), "ready");
}
