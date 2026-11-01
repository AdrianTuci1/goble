//! The product shell.
//!
//! Startup order matters here: telemetry owns the logger, so it is installed
//! before anything can log, and its panic hook is installed before any of the
//! app is built. Both are best effort — if telemetry cannot start, the app
//! still runs with a plain logger and no crash capture.

use std::cell::RefCell;
use std::process::Command;
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

    // Before any window is drawn: a build that was installed and never proved
    // itself is undone here, and the copy it replaced takes over. A broken
    // build therefore costs one launch, not the installation.
    recover_from_broken_update();

    // Some MCP/vault operations (search, install, update, test-call) call
    // `tokio::runtime::Handle::try_current()` and `block_on` on the calling
    // thread. Keep a multi-thread runtime entered for the app lifetime so
    // those calls succeed from the UI thread.
    let runtime = tokio::runtime::Runtime::new()?;
    let _runtime_guard = runtime.enter();

    if let Some(telemetry) = &telemetry {
        telemetry.app_started(locale());
    }
    // The checker starts its own thread and owns its clock: a check now, then
    // at most every six hours. `None` when this build has no release host.
    let updates = goble_app::update::UpdateService::from_env(
        app_version(),
        update_channel(),
        goble_update::UNKNOWN_EPOCH,
    );
    if let Some(service) = &updates {
        service.start();
    }

    // Open the real backend state. If the store genuinely cannot be opened we
    // surface an error instead of silently substituting mock data: the app
    // never runs on a fabricated backend.
    let desktop = DesktopState::open_default()?;
    let bus = CollectingEventBus::new();
    desktop.set_event_bus(Arc::new(bus.clone()));
    // Cron workflows live in the store; this is the clock that fires them. It
    // starts after the bus so a due run's `workflows:updated` is delivered.
    goble_desktop_service::start_workflow_scheduler(&desktop);

    let app_context = Rc::new(RefCell::new(AppContext::default()));
    let root = {
        let ctx = app_context.borrow();
        let root = RootView::new(&ctx, &desktop, Some(bus)).with_app_context(app_context.clone());
        match &updates {
            Some(service) => root.with_updates(Arc::clone(service)),
            None => root,
        }
    };

    // The control socket is a second front door onto the same UI state. Its
    // verbs are queued to the winit thread; `pty.*` is answered by the server
    // thread from the PTY daemon. A socket that cannot bind (another app
    // instance owns it) is logged, never fatal: the window still opens.
    #[cfg(unix)]
    let (root, owns_control) = match goble_app::control::start_default() {
        Ok(control) => {
            log::info!(
                "control socket listening at {}",
                control.socket_path().display()
            );
            (root.with_control(control), true)
        }
        Err(err) => {
            log::warn!("control socket is off for this run: {err}");
            (root, false)
        }
    };
    #[cfg(not(unix))]
    let owns_control = true;

    // The instance that owns the control socket is the one that may sweep the
    // daemon: a second instance shares it and must leave the first one's
    // shells alone. A run that was killed before it could close its panes
    // leaves their sessions detached, and the daemon keeps those alive.
    if owns_control {
        root.prune_orphan_terminal_sessions();
    }
    // The panes in the restored layout get their shells now, before the first
    // frame. Painting a pane must not start a process: a test that renders the
    // shell would otherwise leave a real shell running in the user's daemon.
    root.start_terminal_sessions();
    // Keep a handle after the window is handed to the event loop: a clean quit
    // has to close the pane shells, because dropping a session only detaches.
    let ui_state = root.state_rc();
    // First run shows the three-step onboarding over the window. This is the
    // real-launch seam: mounting the tree in a test never opens it.
    ui_state
        .borrow_mut()
        .open_onboarding_if_first_run(Some(&desktop));

    let result = run_with_root(Box::new(root), app_context);
    ui_state.borrow().kill_terminal_sessions();

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
            // Telemetry could not take the logger, but the app still needs its
            // own sinks: stderr and `~/.goble/logs/goble.log`.
            goble_app::logging::install_default();
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
    // travels with a crash report. The tee writes to stderr and to
    // `~/.goble/logs/goble.log`, so a GUI run keeps its errors after the
    // terminal is gone.
    let logger = goble_app::logging::app_logger();
    let max_level = logger.max_level();
    telemetry.install_logger(Box::new(logger), max_level)?;
    telemetry.clone().install_panic_hook();
    // Publish the handle so outcome sites elsewhere in the app (a finished
    // turn, an approval, a command block) can record without being handed it.
    goble_telemetry::set_global(telemetry.clone());
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

/// Undo an update that was installed and never reached its healthy marker.
///
/// `goble_update::rollback_local` puts the previous copy back and records the
/// failed version; this hands the machine over to the restored build and exits,
/// so the user is not left running the build that did not start cleanly.
fn recover_from_broken_update() {
    let outcome = match goble_update::rollback_local() {
        Ok(outcome) => outcome,
        Err(err) => {
            log::warn!("the update markers could not be read: {err}");
            return;
        }
    };
    let goble_update::LocalRollback::Restored { target, .. } = &outcome else {
        if let Some(message) = outcome.message() {
            log::warn!("{message}");
        }
        return;
    };
    if let Some(message) = outcome.message() {
        log::warn!("{message}");
    }

    let launched = if target.is_dir() {
        Command::new("open").arg("-n").arg(target).spawn()
    } else {
        Command::new(target).spawn()
    };
    match launched {
        Ok(_) => std::process::exit(0),
        Err(err) => log::error!(
            "the previous build is back at {} but could not be started: {err}",
            target.display()
        ),
    }
}
