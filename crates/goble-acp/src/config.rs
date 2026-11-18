//! Environment configuration for the ACP bridge.
//!
//! Everything is read from the environment at startup, exactly like the
//! reference `bridge-acp` implementation: an ACP client such as T3 Code sets
//! these as "environment overrides" on the local-command provider.
//!
//! | Variable | Meaning | Default |
//! |---|---|---|
//! | `GOBLE_GATEWAY_URL` | Gateway base URL | `http://127.0.0.1:8788` |
//! | `GOBLE_GATEWAY_TOKEN` | Bearer token for `/api/v1` | — |
//! | `GOBLE_GATEWAY_PIN` | PIN paired into a token via `POST /api/v1/session` | — |
//! | `GOBLE_ACP_SESSION_NAME` | Fixed gateway session name for every ACP session | derived |
//! | `GOBLE_ACP_WORKER` | Worker id to bind gateway sessions to | gateway default |
//! | `GOBLE_ACP_RUN_TIMEOUT_S` | Run budget sent to the gateway | `1800` |
//! | `GOBLE_ACP_STATE_DIR` | Where `sessions.json` is persisted | `$HOME/.goble/acp` |
//! | `GOBLE_ACP_STRICT_SETUP` | `1` refuses `mcpServers`/extra dirs | unset (warn) |
//! | `GOBLE_ACP_RECONNECT_ATTEMPTS` | SSE reconnect attempts after a drop | `3` |

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};

/// Gateway base used when `GOBLE_GATEWAY_URL` is unset. Matches the gateway's
/// default listen address (`goble-gateway::DEFAULT_BIND`).
pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8788";
/// Default run budget, in seconds, forwarded to the gateway as `timeout_s`.
pub const DEFAULT_RUN_TIMEOUT_S: u64 = 1800;
/// Default number of API-request timeouts before surfacing a transport error.
pub const DEFAULT_RECONNECT_ATTEMPTS: u32 = 3;
/// How long a single non-streaming gateway request may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the one-shot PIN pairing request may take.
pub const PAIR_TIMEOUT: Duration = Duration::from_secs(15);

/// A validated ACP bridge configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcpConfig {
    /// Gateway base URL without a trailing slash, e.g. `http://127.0.0.1:8788`.
    pub gateway_url: String,
    /// Static bearer token (`GOBLE_GATEWAY_TOKEN`), if configured.
    pub token: Option<String>,
    /// PIN paired into a bearer token (`GOBLE_GATEWAY_PIN`), if configured.
    pub pin: Option<String>,
    /// Fixed gateway session name; when set every `session/new` reuses it.
    pub session_name: Option<String>,
    /// Optional worker id the gateway should bind the session to.
    pub worker: Option<String>,
    /// Run budget forwarded to the gateway.
    pub run_timeout_s: u64,
    /// Where the ACP↔gateway session map is persisted. `None` disables resume
    /// across restarts.
    pub state_dir: Option<PathBuf>,
    /// Refuse (rather than warn about) features the gateway cannot mount.
    pub strict_setup: bool,
    /// Number of SSE reconnect attempts after a transport drop.
    pub reconnect_attempts: u32,
}

/// A configuration problem, reported on stderr before the process exits.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("invalid GOBLE_GATEWAY_URL {url:?}: {reason}")]
    InvalidGatewayUrl { url: String, reason: String },
    #[error("invalid GOBLE_ACP_RUN_TIMEOUT_S {value:?}: expected a positive number of seconds")]
    InvalidRunTimeout { value: String },
    #[error("invalid GOBLE_ACP_RECONNECT_ATTEMPTS {value:?}: expected a non-negative integer")]
    InvalidReconnectAttempts { value: String },
    #[error("GOBLE_ACP_STRICT_SETUP must be 0 or 1, got {value:?}")]
    InvalidStrictSetup { value: String },
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            gateway_url: DEFAULT_GATEWAY_URL.to_string(),
            token: None,
            pin: None,
            session_name: None,
            worker: None,
            run_timeout_s: DEFAULT_RUN_TIMEOUT_S,
            state_dir: default_state_dir(),
            strict_setup: false,
            reconnect_attempts: DEFAULT_RECONNECT_ATTEMPTS,
        }
    }
}

impl AcpConfig {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Read the configuration from an arbitrary key/value source. Split out so
    /// tests can exercise parsing without mutating the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut config = Self::default();

        if let Some(raw) = non_empty(lookup("GOBLE_GATEWAY_URL")) {
            config.gateway_url = normalize_gateway_url(&raw)?;
        }
        config.token = non_empty(lookup("GOBLE_GATEWAY_TOKEN"));
        config.pin = non_empty(lookup("GOBLE_GATEWAY_PIN"));
        config.session_name = non_empty(lookup("GOBLE_ACP_SESSION_NAME"));
        config.worker = non_empty(lookup("GOBLE_ACP_WORKER"));

        if let Some(raw) = non_empty(lookup("GOBLE_ACP_RUN_TIMEOUT_S")) {
            let seconds = raw
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(ConfigError::InvalidRunTimeout { value: raw })?;
            config.run_timeout_s = seconds;
        }

        if let Some(raw) = non_empty(lookup("GOBLE_ACP_RECONNECT_ATTEMPTS")) {
            config.reconnect_attempts = raw
                .parse::<u32>()
                .map_err(|_| ConfigError::InvalidReconnectAttempts { value: raw.clone() })?;
        }

        if let Some(raw) = non_empty(lookup("GOBLE_ACP_STRICT_SETUP")) {
            config.strict_setup = match raw.as_str() {
                "1" | "true" | "TRUE" | "yes" => true,
                "0" | "false" | "FALSE" | "no" => false,
                _ => return Err(ConfigError::InvalidStrictSetup { value: raw }),
            };
        }

        if let Some(raw) = non_empty(lookup("GOBLE_ACP_STATE_DIR")) {
            config.state_dir = Some(PathBuf::from(raw));
        }

        Ok(config)
    }

    /// Whether the bridge can authenticate against an auth-enabled gateway.
    pub fn has_auth(&self) -> bool {
        self.token.is_some() || self.pin.is_some()
    }

    /// Path of the durable ACP↔gateway session map, if persistence is enabled.
    pub fn sessions_path(&self) -> Option<PathBuf> {
        self.state_dir.as_ref().map(|dir| dir.join("sessions.json"))
    }

    /// Human-readable authentication mode for logs (never the secret itself).
    pub fn auth_mode(&self) -> &'static str {
        if self.token.is_some() {
            "bearer-token"
        } else if self.pin.is_some() {
            "pin-pairing"
        } else {
            "none"
        }
    }
}

/// A persisted ACP session: the gateway binding plus the metadata needed to
/// resume it after the bridge restarts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSession {
    pub acp_session_id: String,
    pub gateway_session_id: String,
    pub name: String,
    pub cwd: String,
    #[serde(default)]
    pub created_at_ms: u64,
}

/// The on-disk form of `sessions.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedState {
    #[serde(default)]
    pub sessions: Vec<PersistedSession>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

/// Normalise a gateway URL: require an `http`/`https` base, and strip any
/// trailing slash so routes concatenate predictably.
pub fn normalize_gateway_url(raw: &str) -> Result<String, ConfigError> {
    let parsed = Url::parse(raw).map_err(|error| ConfigError::InvalidGatewayUrl {
        url: raw.to_string(),
        reason: error.to_string(),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ConfigError::InvalidGatewayUrl {
            url: raw.to_string(),
            reason: format!(
                "unsupported scheme {:?}; use http or https",
                parsed.scheme()
            ),
        });
    }
    if parsed.cannot_be_a_base() || parsed.host_str().is_none() {
        return Err(ConfigError::InvalidGatewayUrl {
            url: raw.to_string(),
            reason: "missing host".to_string(),
        });
    }
    Ok(raw.trim_end_matches('/').to_string())
}

fn default_state_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".goble").join("acp"))
}

impl fmt::Display for AcpConfig {
    /// Safe to print: contains no secrets.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "gateway={} auth={} state_dir={}",
            self.gateway_url,
            self.auth_mode(),
            self.state_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<disabled>".to_string())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config_from(pairs: &[(&str, &str)]) -> Result<AcpConfig, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        AcpConfig::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn defaults_are_local_and_honest() {
        let config = AcpConfig::from_lookup(|_| None).expect("defaults are valid");
        assert_eq!(config.gateway_url, DEFAULT_GATEWAY_URL);
        assert_eq!(config.run_timeout_s, DEFAULT_RUN_TIMEOUT_S);
        assert_eq!(config.reconnect_attempts, DEFAULT_RECONNECT_ATTEMPTS);
        assert!(!config.has_auth());
        assert!(!config.strict_setup);
    }

    #[test]
    fn reads_every_supported_variable() {
        let config = config_from(&[
            ("GOBLE_GATEWAY_URL", "https://goble.example/"),
            ("GOBLE_GATEWAY_TOKEN", " secret-token "),
            ("GOBLE_GATEWAY_PIN", "ABCD2345"),
            ("GOBLE_ACP_SESSION_NAME", "acp/proj/one"),
            ("GOBLE_ACP_WORKER", "w-1"),
            ("GOBLE_ACP_RUN_TIMEOUT_S", "60"),
            ("GOBLE_ACP_STATE_DIR", "/tmp/goble-acp-tests"),
            ("GOBLE_ACP_STRICT_SETUP", "1"),
            ("GOBLE_ACP_RECONNECT_ATTEMPTS", "0"),
        ])
        .expect("valid config");
        assert_eq!(config.gateway_url, "https://goble.example");
        assert_eq!(config.token.as_deref(), Some("secret-token"));
        assert_eq!(config.pin.as_deref(), Some("ABCD2345"));
        assert_eq!(config.session_name.as_deref(), Some("acp/proj/one"));
        assert_eq!(config.worker.as_deref(), Some("w-1"));
        assert_eq!(config.run_timeout_s, 60);
        assert_eq!(config.reconnect_attempts, 0);
        assert!(config.strict_setup);
        assert_eq!(
            config.sessions_path(),
            Some(PathBuf::from("/tmp/goble-acp-tests/sessions.json"))
        );
        assert!(config.has_auth());
        assert_eq!(config.auth_mode(), "bearer-token");
    }

    #[test]
    fn empty_values_fall_back_to_defaults() {
        let config = config_from(&[
            ("GOBLE_GATEWAY_URL", "   "),
            ("GOBLE_GATEWAY_TOKEN", ""),
            ("GOBLE_ACP_STATE_DIR", ""),
        ])
        .expect("empty values are ignored");
        assert_eq!(config.gateway_url, DEFAULT_GATEWAY_URL);
        assert_eq!(config.token, None);
        assert_eq!(config.state_dir, default_state_dir());
    }

    #[test]
    fn rejects_non_http_gateway_urls() {
        let error = config_from(&[("GOBLE_GATEWAY_URL", "file:///etc/passwd")])
            .expect_err("file URLs are refused");
        assert!(matches!(error, ConfigError::InvalidGatewayUrl { .. }));
        let error =
            config_from(&[("GOBLE_GATEWAY_URL", "not a url")]).expect_err("garbage is refused");
        assert!(matches!(error, ConfigError::InvalidGatewayUrl { .. }));
    }

    #[test]
    fn rejects_invalid_numbers() {
        assert!(matches!(
            config_from(&[("GOBLE_ACP_RUN_TIMEOUT_S", "0")]),
            Err(ConfigError::InvalidRunTimeout { .. })
        ));
        assert!(matches!(
            config_from(&[("GOBLE_ACP_RUN_TIMEOUT_S", "soon")]),
            Err(ConfigError::InvalidRunTimeout { .. })
        ));
        assert!(matches!(
            config_from(&[("GOBLE_ACP_RECONNECT_ATTEMPTS", "-1")]),
            Err(ConfigError::InvalidReconnectAttempts { .. })
        ));
        assert!(matches!(
            config_from(&[("GOBLE_ACP_STRICT_SETUP", "maybe")]),
            Err(ConfigError::InvalidStrictSetup { .. })
        ));
    }

    #[test]
    fn display_never_leaks_secrets() {
        let config = config_from(&[("GOBLE_GATEWAY_TOKEN", "super-secret")]).expect("valid");
        let rendered = config.to_string();
        assert!(rendered.contains("bearer-token"));
        assert!(!rendered.contains("super-secret"));
    }
}
