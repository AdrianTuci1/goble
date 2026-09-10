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

    run_with_root(Box::new(root), app_context)
}
