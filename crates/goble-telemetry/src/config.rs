//! Consent, endpoints and the small amount of state telemetry keeps on disk.
//!
//! Nothing in this module may fail the caller: a missing, unreadable or corrupt
//! state file means "defaults", because a telemetry layer that can break startup
//! or a crash report is worse than no telemetry at all.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The name of the switch that turns uploading on or off.
pub const ENV_ENABLED: &str = "GOBLE_TELEMETRY";
/// Overrides the collector's base URL.
pub const ENV_ENDPOINT: &str = "GOBLE_TELEMETRY_ENDPOINT";
/// The cross-project convention for opting out of telemetry.
pub const ENV_DO_NOT_TRACK: &str = "DO_NOT_TRACK";
/// Where reports go unless something overrides it. This host is not deployed
/// yet; until it is, uploads fail and stay queued on disk.
pub const DEFAULT_ENDPOINT: &str = "https://telemetry.goble.dev";

/// Where a build came from. A developer's working tree is not a user's machine,
/// so the two do not report the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
    #[default]
    Dev,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Dev => "dev",
        }
    }

    /// Whether a fresh installation of this channel uploads by default. Only a
    /// released build reports; a dev build has to be asked.
    pub fn reports_by_default(self) -> bool {
        !matches!(self, Self::Dev)
    }
}

/// Why the upload switch sits where it does, so "why is nothing arriving?" has
/// an answer that does not require guesswork.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentSource {
    Default,
    UserSetting,
    Environment,
    DoNotTrack,
}

impl ConsentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::UserSetting => "user setting",
            Self::Environment => "environment",
            Self::DoNotTrack => "do-not-track",
        }
    }
}

/// Whether reports may leave the machine, and who decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consent {
    pub upload: bool,
    pub source: ConsentSource,
}

impl Consent {
    /// Resolve the switch from the environment, the user's stored choice and the
    /// build's channel, in that order of precedence.
    pub fn resolve(channel: Channel, stored: Option<bool>, env: &dyn Environment) -> Self {
        if env.var(ENV_DO_NOT_TRACK).as_deref().is_some_and(is_truthy) {
            return Self { upload: false, source: ConsentSource::DoNotTrack };
        }
        if let Some(value) = env.var(ENV_ENABLED).as_deref().and_then(parse_bool) {
            return Self { upload: value, source: ConsentSource::Environment };
        }
        match stored {
            Some(upload) => Self { upload, source: ConsentSource::UserSetting },
            None => {
                Self { upload: channel.reports_by_default(), source: ConsentSource::Default }
            }
        }
    }
}

/// Environment lookups, injected so tests do not have to mutate the process.
pub trait Environment: Send + Sync + std::fmt::Debug {
    fn var(&self, name: &str) -> Option<String>;
}

/// The real process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl Environment for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

fn is_truthy(value: &str) -> bool {
    parse_bool(value).unwrap_or(true)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Where telemetry keeps its files inside the Goble home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelemetryPaths {
    root: PathBuf,
}

impl TelemetryPaths {
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The archive of crash reports written on this machine. Kept even when
    /// uploading is off: a user can always attach the file to an issue.
    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("crashes")
    }

    /// Reports and events waiting to be uploaded. Separate from the archive
    /// because entries here are rewritten, retried and deleted.
    pub fn queue_dir(&self) -> PathBuf {
        self.root.join("telemetry-queue")
    }

    /// Install id, channel choice and the user's telemetry setting.
    pub fn state_path(&self) -> PathBuf {
        self.root.join("telemetry.json")
    }

    /// Create the directories telemetry writes to.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.reports_dir())?;
        std::fs::create_dir_all(self.queue_dir())
    }
}

/// What telemetry remembers between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryState {
    /// The user's choice, once they have made one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<bool>,
    /// Anonymous, random, per installation. Not derived from anything about the
    /// machine or the person using it.
    #[serde(default)]
    pub install_id: String,
    /// Whether the first-run notice has been shown.
    #[serde(default)]
    pub notice_shown: bool,
}

impl TelemetryState {
    /// Read the state, or return defaults if it is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(err) => {
                log::warn!("ignoring unreadable telemetry state {}: {err}", path.display());
                Self::default()
            }
        }
    }

    /// Write the state. Best effort: callers log and continue on failure.
    pub fn store(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, path)
    }

    /// The install id, generating and storing one on first use.
    pub fn ensure_install_id(&mut self, path: &Path) -> String {
        if self.install_id.is_empty() {
            self.install_id = uuid::Uuid::new_v4().simple().to_string();
            if let Err(err) = self.store(path) {
                log::warn!("could not persist telemetry state: {err}");
            }
        }
        self.install_id.clone()
    }
}

/// Everything telemetry needs to know before it can start.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    pub app_version: String,
    pub channel: Channel,
    /// Base URL of the collector; `/v1/crashes` and `/v1/events` are appended.
    pub endpoint: String,
    pub consent: Consent,
    pub paths: TelemetryPaths,
    /// Upload attempts per report before leaving it for the next launch.
    pub max_attempts: u32,
    /// Delay before the second attempt; later attempts back off from it.
    pub retry_backoff: Duration,
    /// Reports kept on disk; the oldest are dropped past this.
    pub max_queued: usize,
    /// Reports older than this are dropped.
    pub max_age: Duration,
    /// Per-request timeout.
    pub request_timeout: Duration,
    /// Values scrubbed from every report in addition to the built-in patterns.
    pub secrets: Vec<String>,
    /// Overrides the detected OS name; tests use it.
    pub os: Option<String>,
    pub os_version: Option<String>,
    /// Overrides the detected CPU architecture; tests use it.
    pub arch: Option<String>,
    /// Overrides the generated session id; tests use it.
    pub session_id: Option<String>,
    /// Overrides the locale sent with `AppStarted`.
    pub locale: Option<String>,
}

impl TelemetryConfig {
    pub fn new(
        app_version: impl Into<String>,
        channel: Channel,
        paths: TelemetryPaths,
    ) -> Self {
        Self {
            app_version: app_version.into(),
            channel,
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            consent: Consent { upload: false, source: ConsentSource::Default },
            paths,
            max_attempts: 3,
            retry_backoff: Duration::from_secs(5),
            max_queued: 50,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            request_timeout: Duration::from_secs(10),
            secrets: Vec::new(),
            os: None,
            os_version: None,
            arch: None,
            session_id: None,
            locale: None,
        }
    }

    /// Apply the environment overrides: endpoint, and the consent switch.
    pub fn resolved(mut self, env: &dyn Environment) -> Self {
        if let Some(endpoint) = env.var(ENV_ENDPOINT).map(|v| v.trim().to_owned()) {
            if !endpoint.is_empty() {
                self.endpoint = endpoint;
            }
        }
        let stored = TelemetryState::load(&self.paths.state_path()).upload;
        self.consent = Consent::resolve(self.channel, stored, env);
        self
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_consent(mut self, consent: Consent) -> Self {
        self.consent = consent;
        self
    }

    /// Never upload anything. Reports are still captured on disk.
    pub fn without_upload(mut self) -> Self {
        self.consent = Consent { upload: false, source: ConsentSource::UserSetting };
        self
    }

    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.secrets.push(secret.into());
        self
    }

    /// Collector base URL, or an empty string when uploading is not configured.
    pub fn collector(&self) -> &str {
        self.endpoint.trim_end_matches('/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeEnv(Vec<(&'static str, &'static str)>);

    impl FakeEnv {
        fn with(pairs: &[(&'static str, &'static str)]) -> Self {
            Self(pairs.to_vec())
        }
    }

    impl Environment for FakeEnv {
        fn var(&self, name: &str) -> Option<String> {
            self.0.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn a_release_reports_and_a_dev_build_does_not() {
        let env = FakeEnv::default();
        assert!(Consent::resolve(Channel::Stable, None, &env).upload);
        assert!(Consent::resolve(Channel::Beta, None, &env).upload);
        assert!(!Consent::resolve(Channel::Dev, None, &env).upload);
    }

    #[test]
    fn the_user_setting_beats_the_default() {
        let consent = Consent::resolve(Channel::Stable, Some(false), &FakeEnv::default());
        assert!(!consent.upload);
        assert_eq!(consent.source, ConsentSource::UserSetting);
    }

    #[test]
    fn the_environment_beats_the_user_setting() {
        let env = FakeEnv::with(&[(ENV_ENABLED, "0")]);
        let consent = Consent::resolve(Channel::Stable, Some(true), &env);
        assert!(!consent.upload);
        assert_eq!(consent.source, ConsentSource::Environment);
    }

    #[test]
    fn do_not_track_beats_everything_and_never_opts_in() {
        let on = FakeEnv::with(&[(ENV_DO_NOT_TRACK, "1"), (ENV_ENABLED, "1")]);
        let consent = Consent::resolve(Channel::Stable, Some(true), &on);
        assert!(!consent.upload);
        assert_eq!(consent.source, ConsentSource::DoNotTrack);

        // A `DO_NOT_TRACK=0` is not an opt-in.
        let off = FakeEnv::with(&[(ENV_DO_NOT_TRACK, "0")]);
        let consent = Consent::resolve(Channel::Dev, None, &off);
        assert!(!consent.upload);
        assert_eq!(consent.source, ConsentSource::Default);
    }

    #[test]
    fn unparseable_switches_are_ignored() {
        let env = FakeEnv::with(&[(ENV_ENABLED, "maybe")]);
        let consent = Consent::resolve(Channel::Stable, Some(false), &env);
        assert!(!consent.upload);
        assert_eq!(consent.source, ConsentSource::UserSetting);
    }

    #[test]
    fn the_endpoint_can_be_overridden() {
        let dir = tempfile::tempdir().unwrap();
        let env = FakeEnv::with(&[(ENV_ENDPOINT, "http://127.0.0.1:9/")]);
        let config = TelemetryConfig::new("1.2.3", Channel::Dev, TelemetryPaths::under(dir.path()))
            .with_endpoint(DEFAULT_ENDPOINT)
            .resolved(&env);
        assert_eq!(config.collector(), "http://127.0.0.1:9");
    }

    #[test]
    fn the_install_id_is_stable_across_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = TelemetryPaths::under(dir.path()).state_path();

        let mut state = TelemetryState::load(&path);
        let first = state.ensure_install_id(&path);
        assert!(!first.is_empty());

        let mut reloaded = TelemetryState::load(&path);
        assert_eq!(reloaded.ensure_install_id(&path), first);
    }

    #[test]
    fn user_choices_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = TelemetryPaths::under(dir.path()).state_path();

        let mut state = TelemetryState { upload: Some(false), notice_shown: true, ..Default::default() };
        state.ensure_install_id(&path);
        state.store(&path).unwrap();

        let reloaded = TelemetryState::load(&path);
        assert_eq!(reloaded.upload, Some(false));
        assert!(reloaded.notice_shown);
        assert_eq!(reloaded.install_id, state.install_id);
    }

    #[test]
    fn a_corrupt_state_file_is_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = TelemetryPaths::under(dir.path()).state_path();
        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(TelemetryState::load(&path), TelemetryState::default());
    }
}
