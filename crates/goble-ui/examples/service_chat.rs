use goble_core::store::Store;
use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::{
    AppContext, ChatMessage as UiChatMessage, ChatRole, LayoutContext, SizeConstraint,
};
use goble_ui::geometry::vec2f;
use goble_ui::{ChatView, Element};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let _guard = runtime.enter();

    let bus = CollectingEventBus::default();
    let state = DesktopState::new(
        Store::open_in_memory()?,
        goble_desktop_service::ThreadStore::new(PathBuf::from("/tmp/goble-ui-example"))?,
    );
    state.set_event_bus(Arc::new(bus.clone()));

    let chat_id = state.create_chat("Native chat", None, None)?;
    state.add_chat_message(&chat_id, "user", "Hello from the native UI!")?;
    state.add_chat_message(
        &chat_id,
        "assistant",
        "Hi there! I am backed by the shared service layer.",
    )?;

    let service_messages = state.list_chat_messages(&chat_id)?;
    let ui_messages: Vec<UiChatMessage> = service_messages
        .into_iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "user" => ChatRole::User,
                _ => ChatRole::Assistant,
            };
            UiChatMessage::from_markdown(role, m.content)
        })
        .collect();

    println!(
        "Loaded {} message(s) from service layer.",
        ui_messages.len()
    );

    let app = AppContext::default();
    let mut chat_view = ChatView::new().with_messages(ui_messages);
    let size = chat_view.layout(
        SizeConstraint::loose(vec2f(800.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );

    println!("ChatView layout size: {}x{}", size.x, size.y);
    println!("Service events emitted: {}", bus.events().len());

    Ok(())
}
