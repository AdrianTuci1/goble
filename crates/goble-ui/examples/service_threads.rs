use goble_core::store::Store;
use goble_core::thread::{Participant, ThreadKind, UserId};
use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::{
    AppContext, ChatMessage as UiChatMessage, LayoutContext, SizeConstraint,
};
use goble_ui::geometry::vec2f;
use goble_ui::{Element, ThreadKind as UiThreadKind, ThreadListEntry, ThreadsContainer};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();

    let bus = CollectingEventBus::default();
    let state = DesktopState::new(
        Store::open_in_memory()?,
        goble_desktop_service::ThreadStore::new(PathBuf::from("/tmp/goble-ui-threads-example"))?,
    );
    state.set_event_bus(Arc::new(bus.clone()));

    let owner = UserId::generate();
    let thread = state
        .thread_store()
        .create_thread(
            ThreadKind::Channel,
            "General",
            owner.clone(),
            false,
            vec![Participant::User(owner.clone())],
            vec!["#general".to_string()],
        )?;

    state.thread_store().post_message(
        &thread.id,
        Participant::User(owner.clone()),
        "Hello **threads**!",
        None,
        vec![],
        vec![],
        None,
    )?;
    state.thread_store().post_message(
        &thread.id,
        Participant::User(owner.clone()),
        "This message is rendered in the native UI.",
        None,
        vec![],
        vec![],
        None,
    )?;

    let threads = vec![ThreadListEntry {
        id: thread.id.0.clone(),
        title: thread.title.clone(),
        kind: UiThreadKind::Channel,
        selected: true,
        unread_count: 0,
    }];

    let messages: Vec<UiChatMessage> = state
        .thread_store()
        .list_messages(&thread.id)?
        .iter()
        .map(UiChatMessage::from_thread_message)
        .collect();

    println!("Loaded {} thread(s) with {} message(s).", threads.len(), messages.len());

    let app = AppContext::default();
    let mut container = ThreadsContainer::new(&thread.id.0)
        .with_threads(threads)
        .with_messages(&thread.id.0, messages)
        .with_on_send(|text| println!("send: {}", text))
        .with_on_select(|id| println!("selected thread: {}", id));

    let size = container.layout(
        SizeConstraint::loose(vec2f(900.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );

    println!("ThreadsContainer layout size: {}x{}", size.x, size.y);
    println!("Service events emitted: {}", bus.events().len());

    Ok(())
}
