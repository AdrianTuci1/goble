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
    // DEV scaffolding: seed demo conversations so the sidebar and chat
    // have content to render on first run. Remove this call (and the
    // `seed_demo_conversations` fn) once real data flows in.
    if let Err(e) = seed_demo_conversations(&desktop) {
        log::warn!("failed to seed demo conversations: {e}");
    }

    let app_context = Rc::new(RefCell::new(AppContext::default()));
    let root = {
        let ctx = app_context.borrow();
        RootView::new(&ctx, &desktop, Some(bus))
    };

    run_with_root(Box::new(root), app_context)
}

/// DEV scaffolding: create a couple of demo conversations the first time the
/// app runs against an empty store, so the sidebar has cards and the chat has
/// something to render. Remove this function and its call in `main` to stop
/// seeding. The second conversation exercises custom fragment renders
/// (heading, list, code block, blockquote, link) via Markdown.
fn seed_demo_conversations(desktop: &DesktopState) -> anyhow::Result<()> {
    if !desktop.list_chats().is_empty() {
        return Ok(());
    }

    // Created first so it is the default selected chat (custom renders).
    let renders = desktop.create_chat("Demo · Custom renders", None, None)?;
    desktop.add_chat_message(&renders, "user", "Arată-mi un răspuns bogat.")?;
    desktop.add_chat_message(
        &renders,
        "assistant",
        "# Pași de lucru\n\nAcesta e un exemplu de `rendere personalizate`:\n\n1. Heading\n2. Listă\n3. Bloc de cod\n4. Citat\n5. Link\n\n> Un citat cu stil propriu.\n\n```rust\nfn main() {\n    println!(\"salut\");\n}\n```\n\nTotul e **bold** și [un link](https://goble.dev).",
    )?;

    let runtime = desktop.create_chat("Demo · Runtime local", None, None)?;
    desktop.add_chat_message(&runtime, "user", "Unde rulează turn-urile mele?")?;
    desktop.add_chat_message(
        &runtime,
        "assistant",
        "Chat-ul rulează pe harness-ul din `goble-core`, orchestrat din `app` — la fel cum warp-new ține runtime-ul în app, nu într-un crate separat.",
    )?;

    Ok(())
}
