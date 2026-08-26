use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::AppContext;
use goble_ui::platform::run_with_root;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Some MCP/vault operations (search, install, update, test-call) call
    // `tokio::runtime::Handle::try_current()` and `block_on` on the calling
    // thread. Keep a multi-thread runtime entered for the app lifetime so
    // those calls succeed from the UI thread.
    let runtime = tokio::runtime::Runtime::new()?;
    let _runtime_guard = runtime.enter();

    // Open the real backend state. If the store cannot be opened we fall back
    // to mock data so the UI shell stays runnable during development.
    let (desktop, event_bus) = match DesktopState::open_default() {
        Ok(state) => {
            let bus = CollectingEventBus::new();
            state.set_event_bus(Arc::new(bus.clone()));
            // DEV scaffolding: seed demo routines so the sidebar has cards to
            // render on first run. Remove this call (and the
            // `seed_demo_routines` fn) once real data flows in.
            if let Err(e) = seed_demo_routines(&state) {
                log::warn!("failed to seed demo routines: {e}");
            }
            (Some(state), Some(bus))
        }
        Err(e) => {
            log::warn!("failed to open DesktopState: {e}; running with mock data");
            (None, None)
        }
    };

    let app_context = Rc::new(RefCell::new(AppContext::default()));
    let root = {
        let ctx = app_context.borrow();
        RootView::new(&ctx, desktop, event_bus)
    };

    run_with_root(Box::new(root), app_context)
}

/// DEV scaffolding: create a couple of demo routines the first time the app
/// runs against an empty store, so the sidebar has cards to render. Remove
/// this function and its call in `main` to stop seeding.
fn seed_demo_routines(desktop: &DesktopState) -> anyhow::Result<()> {
    if !desktop.list_workflows().is_empty() {
        return Ok(());
    }

    use goble_core::agent::Trigger;

    // Created first so it is the default selected routine.
    desktop.create_workflow(
        "Demo · Daily summary",
        "Custom renders and rich fragments",
        vec![],
        Trigger::Cron {
            expression: "0 9 * * *".to_string(),
        },
    )?;

    desktop.create_workflow(
        "Demo · Runtime local",
        "Run turn orchestration on the local harness",
        vec![],
        Trigger::Manual,
    )?;

    Ok(())
}
