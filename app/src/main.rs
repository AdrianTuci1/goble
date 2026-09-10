//! The product shell.
//!
//! Startup order matters here: telemetry owns the logger, so it is installed
//! before anything can log, and its panic hook is installed before any of the
//! app is built. Both are best effort — if telemetry cannot start, the app
//! still runs with a plain logger and no crash capture.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use goble_app::root_view::RootView;
use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_telemetry::{Channel, ProcessEnv, Telemetry, TelemetryConfig, TelemetryPaths};
use goble_ui::elements::AppContext;
use goble_ui::platform::run_with_root;

/// How long the shutdown drain may take before the queue is left for next time.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(2);

fn main() -> anyhow::Result<()> {
    let telemetry = start_telemetry();

    // Some MCP/vault operations (search, install, update, test-call) call
    // `tokio::runtime::Handle::try_current()` and `block_on` on the calling
    // thread. Keep a multi-thread runtime entered for the app lifetime so
    // those calls succeed from the UI thread.
    let runtime = tokio::runtime::Runtime::new()?;
    let _runtime_guard = runtime.enter();

    if let Some(telemetry) = &telemetry {
        telemetry.app_started(locale());
    }
    check_for_updates();

    // Open the real backend state. If the store genuinely cannot be opened we
    // surface an error instead of silently substituting mock data: the app
    // never runs on a fabricated backend.
    let desktop = DesktopState::open_default()?;
    let bus = CollectingEventBus::new();
    desktop.set_event_bus(Arc::new(bus.clone()));

    let app_context = Rc::new(RefCell::new(AppContext::default()));
    let root = {
        let ctx = app_context.borrow();
        RootView::new(&ctx, &desktop, Some(bus)).with_app_context(app_context.clone())
    };

    let result = run_with_root(Box::new(root), app_context);

    if let Some(telemetry) = &telemetry {
        telemetry.app_exited(Some(if result.is_ok() { 0 } else { 1 }));
        telemetry.shutdown(SHUTDOWN_DRAIN);
    }
    result
}

/// Start crash reporting and telemetry, and hand it the logger.
///
/// Returns `None` when telemetry could not start; the caller falls back to a
/// plain logger. A telemetry layer that can prevent the app from starting is
/// worse than no telemetry layer.
fn start_telemetry() -> Option<Arc<Telemetry>> {
    match try_start_telemetry() {
        Ok(telemetry) => Some(telemetry),
        Err(err) => {
            env_logger::init();
            log::warn!("telemetry is off for this run: {err}");
            None
        }
    }
}

fn try_start_telemetry() -> Result<Arc<Telemetry>, Box<dyn std::error::Error>> {
    let home = goble_core::app_home::home_dir()?;
    let config = TelemetryConfig::new(app_version(), build_channel(), TelemetryPaths::under(home))
        .resolved(&ProcessEnv);
    let telemetry = Telemetry::start(config)?;

    // Telemetry must own the logger, so it can keep the recent log tail that
    // travels with a crash report. `env_logger` still does the writing.
    let logger = env_logger::Builder::from_default_env().build();
    let max_level = logger.filter();
    telemetry.install_logger(Box::new(logger), max_level)?;
    telemetry.clone().install_panic_hook();
    Ok(telemetry)
}

/// Which release line this build belongs to.
///
/// Baked in at compile time: `packaging/` and the release workflow set
/// `GOBLE_CHANNEL` for a real release, and anything else — a developer's
/// `cargo build`, a working tree — is a dev build. A dev build neither reports
/// crashes nor replaces itself by default.
fn build_channel() -> Channel {
    match option_env!("GOBLE_CHANNEL").unwrap_or("dev") {
        "stable" => Channel::Stable,
        "beta" => Channel::Beta,
        _ => Channel::Dev,
    }
}

/// The version a crash report and an update check compare against.
///
/// `packaging/` sets `GOBLE_RELEASE_VERSION` from the release tag, so a shipped
/// binary does not keep claiming to be the `0.1.0` in `Cargo.toml`.
fn app_version() -> &'static str {
    option_env!("GOBLE_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The same channel, in the update crate's vocabulary.
fn update_channel() -> goble_update::Channel {
    match build_channel() {
        Channel::Stable => goble_update::Channel::Stable,
        Channel::Beta => goble_update::Channel::Beta,
        Channel::Dev => goble_update::Channel::Dev,
    }
}

/// The user's locale, without the encoding part: `en_US.UTF-8` becomes `en_US`.
fn locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty() && value.as_str() != "C")
        .map(|value| value.split(['.', '@']).next().unwrap_or(&value).to_owned())
}

/// An advisory update check.
///
/// Off unless `GOBLE_UPDATE_MANIFEST` names a manifest, because there is nothing
/// to check against until the release host exists, and the app should not open
/// connections to a host that is not there. The outcome is logged; surfacing it
/// in the UI is still open (see `.agents/07-observability/`).
fn check_for_updates() {
    let Ok(manifest_url) = std::env::var("GOBLE_UPDATE_MANIFEST") else {
        return;
    };

    std::thread::Builder::new()
        .name("goble-update-check".to_owned())
        .spawn(move || {
            let mut config = goble_update::UpdateConfig::new(manifest_url);
            if let Ok(key) = std::env::var("GOBLE_UPDATE_PUBLIC_KEY") {
                config = config.with_public_key(key);
            }

            let client = match goble_update::UpdateClient::new(config, "goble-app") {
                Ok(client) => client,
                Err(err) => {
                    log::warn!("update check could not start: {err}");
                    return;
                }
            };

            match client.check(
                update_channel(),
                app_version(),
                std::env::consts::OS,
                std::env::consts::ARCH,
            ) {
                Ok(outcome) => log::info!("update check: {outcome:?}"),
                Err(err) => log::warn!("update check failed: {err}"),
            }
        })
        .ok();
}
