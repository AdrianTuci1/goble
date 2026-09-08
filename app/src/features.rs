//! Runtime feature flags for the app shell.
//!
//! Compile-time Cargo features change the dependency graph (e.g. `daemon`
//! pulls in the remote WebSocket codec). This module is orthogonal: it tracks a
//! few runtime-tunable behaviors so a single build can be exercised both ways
//! without a recompile. Flags are set once from the environment; a value can be
//! overridden before first use via the [`FeatureFlags`] builder (see
//! [`with_enabled`] for the read-side helper).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// The app's runtime feature flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureFlag {
    /// The daemon seam (local embedded daemon or remote) routes chat turns.
    ///
    /// When on, a conversation routed `remote` is **not** silently run locally:
    /// it errors with a clear "not configured" message unless a remote daemon
    /// client was actually constructed. This surfaces the gap instead of hiding
    /// it behind a locally-executed turn.
    Daemon,
}

impl FeatureFlag {
    fn env(&self) -> &'static str {
        match self {
            // `GOBLE_FEATURE_DAEMON=0` disables the daemon-seam path.
            FeatureFlag::Daemon => "GOBLE_FEATURE_DAEMON",
        }
    }

    fn default_on(&self) -> bool {
        match self {
            // The daemon split is the default composition: the desktop service
            // owns an embedded daemon and drives execution through it.
            FeatureFlag::Daemon => true,
        }
    }
}

/// Mutable flag store. Private; access it through the free functions below.
struct FlagStore {
    daemon: AtomicBool,
}

static STORE: OnceLock<FlagStore> = OnceLock::new();

fn store() -> &'static FlagStore {
    STORE.get_or_init(|| {
        // A flag is on unless the environment explicitly disables it, so a
        // clean build gets the intended defaults without extra config.
        FlagStore {
            daemon: AtomicBool::new(env_bool(FeatureFlag::Daemon)),
        }
    })
}

/// Read a flag's environment variable; a missing value falls back to the flag's
/// default, and `0`/`off`/`false` (case-insensitive) disable it.
fn env_bool(flag: FeatureFlag) -> bool {
    match std::env::var(flag.env()) {
        Ok(v) => !(v == "0" || v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("false")),
        Err(_) => flag.default_on(),
    }
}

/// Forcibly initialize the flag store from the environment. Idempotent; a value
/// already set (or a previous call) is not overwritten. Calling this is
/// optional — [`feature_enabled`] lazily initializes on first read.
pub fn init_feature_flags() {
    let _ = store();
}

/// Whether a runtime feature flag is currently enabled.
pub fn feature_enabled(flag: FeatureFlag) -> bool {
    match flag {
        FeatureFlag::Daemon => store().daemon.load(Ordering::Relaxed),
    }
}

/// Run `f` only when `flag` is enabled, returning its result; `None` otherwise.
pub fn with_enabled<T>(flag: FeatureFlag, f: impl FnOnce() -> T) -> Option<T> {
    feature_enabled(flag).then(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_flag_defaults_on() {
        assert!(feature_enabled(FeatureFlag::Daemon), "daemon is the default composition");
        assert!(with_enabled(FeatureFlag::Daemon, || 42).is_some());
    }
}
