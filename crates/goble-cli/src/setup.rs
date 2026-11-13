//! `goble setup`: the write surface an agent-led setup conversation uses.
//!
//! An agent can read `~/.goble/config.toml`, but it should not hand-edit it:
//! the file is written by one serializer, carries a documenting header, and
//! holds sections other tools own. These verbs are the supported way to change
//! it, and they answer with what is still missing instead of guessing. The same
//! verbs are named in the brief `goble setup brief` prints.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use goble_core::app_home::GobleHome;
use goble_core::config::{
    GobleConfig, HubConfig, RemoteAgentConfig, RemoteMode, SetupStatus, SetupTarget,
};

use crate::cli::{SetupAction, SetupTargetArg};

/// The hub's default workflow directory, relative to the project.
const DEFAULT_WORKFLOWS: &str = ".goble/workflows";

pub fn run(config_override: Option<PathBuf>, action: SetupAction) -> Result<()> {
    let home = GobleHome::locate()?;
    let config_path = config_override.unwrap_or_else(|| home.config_path());
    let hub_path = home.hub_config_path();

    match action {
        SetupAction::Status { json } => {
            let status = status(&config_path, &hub_path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print_report(&status);
            }
        }
        SetupAction::Remote {
            name,
            provider,
            api_key,
            token_id,
            token_secret,
            api_url,
            image,
            region,
            environment,
            disabled,
        } => {
            let name = name.trim().to_string();
            if name.is_empty() {
                bail!("the agent needs a name: pass --name <builder|reviewer|…>");
            }
            let agent = RemoteAgentConfig {
                provider: provider.into(),
                enabled: !disabled,
                api_key: clean(api_key),
                token_id: clean(token_id),
                token_secret: clean(token_secret),
                api_url: clean(api_url),
                image: clean(image),
                region: clean(region),
                environment: clean(environment),
            };
            if agent.image.is_none() {
                eprintln!(
                    "note: no --image yet; deploying needs the image that carries goblin-worker"
                );
            }
            let label = upsert_remote(&config_path, &name, agent)?;
            println!("remote agent {name}: {label}");
            print_report(&status(&config_path, &hub_path)?);
        }
        SetupAction::Gateway {
            url,
            token,
            platform,
        } => {
            let label = set_gateway(&config_path, clean(url), clean(token), platform)?;
            println!("{label}");
            print_report(&status(&config_path, &hub_path)?);
        }
        SetupAction::Hub {
            database,
            relay_url,
            token,
            workflows,
        } => {
            let mut hub = HubConfig::load(&hub_path)?.unwrap_or_default();
            if let Some(value) = clean(database) {
                hub.database = Some(value);
            }
            if let Some(value) = clean(relay_url) {
                hub.relay_url = Some(value);
            }
            if let Some(value) = clean(token) {
                hub.token = Some(value);
            }
            if let Some(value) = clean(workflows) {
                hub.workflows = Some(value);
            }
            let label = save_hub(&hub_path, hub, &home.root().join("hub.db"))?;
            println!("{label}");
            print_report(&status(&config_path, &hub_path)?);
        }
        SetupAction::Brief { target } => {
            let status = status(&config_path, &hub_path)?;
            let cli = goble_core::binaries::goble_cli().display().to_string();
            let target: SetupTarget = SetupTargetArg::from(target).into();
            println!("{}", status.brief(target, &cli));
        }
    }
    Ok(())
}

fn print_report(status: &SetupStatus) {
    println!("config: {}", status.config_path);
    for line in status.lines() {
        println!("{line}");
    }
    println!();
    println!(
        "Remote: {}",
        if status.remote_configured() {
            "configured"
        } else {
            "not configured yet"
        }
    );
}

fn status(config_path: &Path, hub_path: &Path) -> Result<SetupStatus> {
    let config = load_config(config_path)?;
    let hub = HubConfig::load(hub_path)?;
    let env: BTreeMap<String, String> = std::env::vars().collect();
    Ok(SetupStatus::from_config(
        &config,
        config_path,
        hub_path,
        hub.as_ref(),
        &env,
    ))
}

/// Read the config, keeping whatever sections parse. A missing file is the
/// empty config, so setup works on a fresh home.
fn load_config(path: &Path) -> Result<GobleConfig> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (config, problems) = GobleConfig::load_toml(&text)?;
            for problem in problems {
                eprintln!("warning: {problem}");
            }
            Ok(config)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(GobleConfig::default()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

fn save_config(path: &Path, config: &GobleConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(path, config.to_toml()?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Write one `[remote_agents.<name>]` table and answer with its status.
fn upsert_remote(config_path: &Path, name: &str, agent: RemoteAgentConfig) -> Result<String> {
    let mut config = load_config(config_path)?;
    let label = format!("{} · {}", agent.provider.as_str(), agent.status_label());
    config.remote_agents.insert(name.to_string(), agent);
    save_config(config_path, &config)?;
    Ok(label)
}

/// Point `[remote]` at a gateway, or back at the platforms. Returns the line to
/// print.
fn set_gateway(
    config_path: &Path,
    url: Option<String>,
    token: Option<String>,
    platform: bool,
) -> Result<String> {
    let mut config = load_config(config_path)?;
    if platform {
        config.remote.mode = RemoteMode::Platform;
        save_config(config_path, &config)?;
        return Ok("remote: platform · the sandbox tables run the turns".to_string());
    }
    let url = url.or_else(|| config.remote.gateway_url().map(str::to_string));
    let token = token.or_else(|| config.remote.worker_token().map(str::to_string));
    let (Some(url), Some(token)) = (url, token) else {
        bail!("a gateway needs both --url and --token; nothing was written");
    };
    config.remote.mode = RemoteMode::Gateway;
    config.remote.gateway_url = Some(url.clone());
    config.remote.worker_token = Some(token);
    save_config(config_path, &config)?;
    Ok(format!("remote: gateway {url} · worker token set"))
}

/// Write `hub.toml`, filling the two defaults the hub can start from.
fn save_hub(hub_path: &Path, mut hub: HubConfig, default_database: &Path) -> Result<String> {
    if hub.database.is_none() {
        hub.database = Some(default_database.display().to_string());
    }
    if hub.workflows.is_none() {
        hub.workflows = Some(DEFAULT_WORKFLOWS.to_string());
    }
    hub.save(hub_path)?;
    Ok(format!("wrote {} · hub {}", hub_path.display(), hub.status_label()))
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::config::RemoteAgentProvider;

    /// A temp home: the two files setup reads and writes, and nothing else.
    fn home() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let config = dir.path().join("config.toml");
        let hub = dir.path().join("hub.toml");
        (dir, config, hub)
    }

    fn e2b() -> RemoteAgentConfig {
        RemoteAgentConfig {
            provider: RemoteAgentProvider::E2b,
            enabled: true,
            api_key: None,
            token_id: None,
            token_secret: None,
            api_url: None,
            image: Some("ghcr.io/goble/worker:latest".to_string()),
            region: None,
            environment: None,
        }
    }

    #[test]
    fn an_agent_table_is_written_beside_the_header_and_reports_its_missing_key() {
        let (_dir, path, _hub) = home();
        let label = upsert_remote(&path, "builder", e2b()).expect("write");
        assert_eq!(label, "e2b · missing E2B_API_KEY");

        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.starts_with(goble_core::config::CONFIG_HEADER), "{text}");
        assert!(text.contains("[remote_agents.builder]"), "{text}");
        assert!(text.contains(r#"provider = "e2b""#), "{text}");

        // A second write to the same name replaces it; other tables and other
        // sections stay.
        let mut docker = e2b();
        docker.provider = RemoteAgentProvider::Docker;
        docker.image = Some("goble-worker:dev".to_string());
        upsert_remote(&path, "builder", docker).expect("rewrite");
        let config = load_config(&path).expect("reload");
        assert_eq!(
            config.remote_agents["builder"].provider,
            RemoteAgentProvider::Docker
        );
        assert_eq!(
            config.remote_agents["builder"].image.as_deref(),
            Some("goble-worker:dev")
        );
    }

    #[test]
    fn a_half_named_gateway_is_refused_and_the_file_keeps_its_old_shape() {
        let (_dir, path, _hub) = home();
        let before = std::fs::read_to_string(&path).unwrap_or_default();
        let error = set_gateway(&path, Some("https://gateway.example.com".to_string()), None, false)
            .expect_err("no token");
        assert!(error.to_string().contains("--url and --token"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), before);
    }

    #[test]
    fn a_gateway_is_written_and_platform_flips_back() {
        let (_dir, path, _hub) = home();
        let label = set_gateway(
            &path,
            Some("https://gateway.example.com".to_string()),
            Some("wt-1".to_string()),
            false,
        )
        .expect("write");
        assert!(label.contains("gateway https://gateway.example.com"), "{label}");

        let config = load_config(&path).expect("reload");
        assert_eq!(config.remote.mode, RemoteMode::Gateway);
        assert_eq!(
            config.remote.gateway_url(),
            Some("https://gateway.example.com")
        );
        assert_eq!(config.remote.worker_token(), Some("wt-1"));

        set_gateway(&path, None, None, true).expect("flip back");
        let config = load_config(&path).expect("reload");
        assert_eq!(config.remote.mode, RemoteMode::Platform);
        assert_eq!(config.remote.worker_token(), Some("wt-1"));
    }

    #[test]
    fn the_hub_file_gets_its_two_defaults() {
        let (_dir, _config, hub_path) = home();
        let hub = HubConfig {
            relay_url: Some("wss://relay.example.com".to_string()),
            token: Some("hub-token".to_string()),
            ..HubConfig::default()
        };
        let label = save_hub(&hub_path, hub, &_dir.path().join("hub.db")).expect("write");
        assert!(label.contains("hub ready"), "{label}");

        let saved = HubConfig::load(&hub_path).expect("load").expect("written");
        assert_eq!(saved.relay_url(), Some("wss://relay.example.com"));
        assert!(saved.has_token());
        assert!(saved
            .database
            .as_deref()
            .is_some_and(|path| path.ends_with("hub.db")));
        assert_eq!(saved.workflows.as_deref(), Some(DEFAULT_WORKFLOWS));
    }

    #[test]
    fn a_fresh_home_status_reports_nothing_configured() {
        let (_dir, config, hub) = home();
        let status = status(&config, &hub).expect("status");
        assert!(!status.remote_configured());
        assert!(status.agents.is_empty());
        assert!(status.hub.is_none());
    }
}
