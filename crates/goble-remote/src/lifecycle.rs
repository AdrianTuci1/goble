//! When a project's sandbox is kept, snapshotted and torn down.
//!
//! The durable-session lifecycle has three configurable knobs:
//!
//! - how long a sandbox may sit idle before the reaper snapshots and destroys
//!   it ([`IDLE_TIMEOUT_ENV`], default 30 minutes; `0` disables reaping);
//! - whether a close snapshots first at all ([`SNAPSHOT_ON_CLOSE_ENV`], default
//!   on) — a provider that cannot snapshot ignores it and destroys normally;
//! - the provider's own sandbox timeout, which stays on
//!   [`crate::SandboxRequest::timeout_secs`].
//!
//! The knobs live in the environment rather than in `config.toml` because the
//! reaper runs in the desktop service, which already reads its credentials and
//! worker token from the environment-adjacent config; a per-table field would
//! have to be added to the shared config schema for a setting that only the
//! provisioner reads.

use std::time::Duration;

/// Environment variable naming the idle timeout in seconds. `0` disables the
/// reaper; unset uses [`DEFAULT_IDLE_TIMEOUT_SECS`]; an unparseable value is
/// also treated as the default.
pub const IDLE_TIMEOUT_ENV: &str = "GOBLE_SANDBOX_IDLE_SECS";

/// Environment variable deciding whether a close snapshots before destroying.
/// `0`, `false` and `no` turn it off; anything else (or unset) leaves it on.
pub const SNAPSHOT_ON_CLOSE_ENV: &str = "GOBLE_SANDBOX_SNAPSHOT_ON_CLOSE";

/// The idle timeout a state uses when the environment does not say otherwise:
/// thirty minutes, the same window the reference control plane uses.
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 30 * 60;

/// How a provisioner keeps and stops its project sandboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxLifecycle {
    /// The idle window before a reaper snapshots and destroys a sandbox.
    /// `None` means the reaper is disabled.
    idle_timeout: Option<Duration>,
    /// Whether a close snapshots before it destroys.
    snapshot_on_close: bool,
}

impl SandboxLifecycle {
    /// A lifecycle with an explicit idle window and close behaviour.
    pub fn new(idle_timeout: Option<Duration>, snapshot_on_close: bool) -> Self {
        Self {
            idle_timeout,
            snapshot_on_close,
        }
    }

    /// The lifecycle the environment names, falling back to the defaults for
    /// every knob it does not set.
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var(IDLE_TIMEOUT_ENV).ok().as_deref(),
            std::env::var(SNAPSHOT_ON_CLOSE_ENV).ok().as_deref(),
        )
    }

    /// [`Self::from_env`] over the raw values, so a test does not mutate the
    /// process environment.
    pub fn from_values(idle_timeout_secs: Option<&str>, snapshot_on_close: Option<&str>) -> Self {
        Self {
            idle_timeout: match idle_timeout_secs.map(str::trim) {
                None | Some("") => Some(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS)),
                Some(raw) => match raw.parse::<u64>() {
                    Ok(0) => None,
                    Ok(secs) => Some(Duration::from_secs(secs)),
                    Err(_) => Some(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS)),
                },
            },
            snapshot_on_close: snapshot_on_close
                .map(str::trim)
                .map(|value| {
                    !matches!(
                        value.to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
        }
    }

    /// The idle window, or `None` when the reaper is disabled.
    pub fn idle_timeout(&self) -> Option<Duration> {
        self.idle_timeout
    }

    /// The idle window in whole seconds, or `None` when the reaper is disabled.
    pub fn idle_timeout_secs(&self) -> Option<u64> {
        self.idle_timeout.map(|timeout| timeout.as_secs())
    }

    /// Whether a close snapshots before it destroys.
    pub fn snapshots_on_close(&self) -> bool {
        self.snapshot_on_close
    }

    /// Whether a sandbox last used at `last_used` is now past the idle window.
    ///
    /// `now` and `last_used` are seconds since the Unix epoch, so the caller
    /// can compare values it read from persistence without building a clock.
    pub fn is_idle(&self, now: i64, last_used: i64) -> bool {
        match self.idle_timeout {
            Some(idle) => now.saturating_sub(last_used) >= idle.as_secs() as i64,
            None => false,
        }
    }
}

impl Default for SandboxLifecycle {
    fn default() -> Self {
        Self {
            idle_timeout: Some(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS)),
            snapshot_on_close: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_lifecycle_reaps_after_thirty_minutes_and_snapshots_on_close() {
        let lifecycle = SandboxLifecycle::default();
        assert_eq!(lifecycle.idle_timeout_secs(), Some(1800));
        assert!(lifecycle.snapshots_on_close());
        assert!(lifecycle.is_idle(1800, 0));
        assert!(!lifecycle.is_idle(1799, 0));
    }

    #[test]
    fn an_idle_timeout_of_zero_disables_the_reaper() {
        let lifecycle = SandboxLifecycle::from_values(Some("0"), None);
        assert_eq!(lifecycle.idle_timeout_secs(), None);
        assert!(!lifecycle.is_idle(i64::MAX, 0));
    }

    #[test]
    fn the_environment_values_are_parsed_with_the_documented_fallbacks() {
        assert_eq!(
            SandboxLifecycle::from_values(Some("600"), Some("false")).idle_timeout_secs(),
            Some(600)
        );
        assert!(!SandboxLifecycle::from_values(None, Some("false")).snapshots_on_close());
        assert!(!SandboxLifecycle::from_values(None, Some("0")).snapshots_on_close());
        assert!(!SandboxLifecycle::from_values(None, Some("NO")).snapshots_on_close());
        assert!(SandboxLifecycle::from_values(None, Some("true")).snapshots_on_close());
        assert!(SandboxLifecycle::from_values(None, None).snapshots_on_close());

        // An unset or blank window is the default; a bad one is the default too.
        assert_eq!(
            SandboxLifecycle::from_values(None, None).idle_timeout_secs(),
            Some(DEFAULT_IDLE_TIMEOUT_SECS)
        );
        assert_eq!(
            SandboxLifecycle::from_values(Some(" "), None).idle_timeout_secs(),
            Some(DEFAULT_IDLE_TIMEOUT_SECS)
        );
        assert_eq!(
            SandboxLifecycle::from_values(Some("soon"), None).idle_timeout_secs(),
            Some(DEFAULT_IDLE_TIMEOUT_SECS)
        );
    }
}
