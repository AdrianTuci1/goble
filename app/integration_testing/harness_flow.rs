//! Integration tests for the harness chat-turn wiring: with a configured LLM,
//! sending a message drives the real harness — a deterministic `MockProvider`
//! in tests, so no network — which persists the user + assistant/tool messages
//! and emits `chat:updated` so the UI refreshes. The no-key path keeps the
//! canned reply.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use goble_app::actions::make_actions;
use goble_app::hot_ui::UiActions;
use goble_app::state::UiState;
use goble_desktop_service::DesktopState;
use goble_ui::{ChatFragmentKind, ChatRole};

const MOCK_REPLY: &str = "No LLM provider configured or API key missing. Add one in Settings.";
const CANNED_REPLY: &str = "Am primit mesajul tău. Rulez acum comanda pentru „Say hi”.";

fn build(desktop: &std::sync::Arc<DesktopState>) -> (Rc<RefCell<UiState>>, UiActions) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let actions = make_actions(Rc::clone(&state), Some(std::sync::Arc::clone(desktop)));
    (state, actions)
}

/// Poll the store until `chat_id` has at least `expected` messages or time out.
fn wait_for_messages(desktop: &DesktopState, chat_id: &str, expected: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Ok(msgs) = desktop.list_chat_messages(chat_id) {
            if msgs.len() >= expected {
                return;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {expected} messages in {chat_id}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A real, deterministic turn on a background task: provider "mock" resolves to
/// the `MockProvider`, so this hits no network. The reply is appended and the
/// UI would refresh via the `chat:updated` event.
#[test]
fn run_chat_turn_appends_mock_reply() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
    desktop
        .add_chat_message(&chat_id, "user", "Hello from test")
        .expect("add user");

    let rt = tokio::runtime::Runtime::new().expect("build runtime");
    let _guard = rt.enter();
    let handle = desktop
        .run_chat_turn(&chat_id, "Say hi", "mock", "")
        .expect("run turn");
    rt.block_on(handle).expect("task completes");

    let messages = desktop.list_chat_messages(&chat_id).expect("list messages");
    let last = messages.last().expect("assistant reply appended");
    assert_eq!(last.role, "assistant");
    assert_eq!(last.content, MOCK_REPLY, "reply should come from the provider, not the canned string");
}

/// The harness task emits a `chat:updated` event after it writes messages, so
/// the UI re-reads the transcript as the turn progresses (rather than waiting
/// for the caller to explicitly refresh).
#[test]
fn run_chat_turn_emits_chat_updated() {
    use std::sync::Arc;
    use goble_desktop_service::CollectingEventBus;

    let (desktop, _dir) = common::desktop_state();
    let concrete = Arc::new(CollectingEventBus::new());
    desktop.set_event_bus(concrete.clone());
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");

    let rt = tokio::runtime::Runtime::new().expect("build runtime");
    let _guard = rt.enter();
    let handle = desktop
        .run_chat_turn(&chat_id, "Say hi", "mock", "")
        .expect("run turn");
    rt.block_on(handle).expect("task completes");

    assert!(
        concrete.has_event("chat:updated"),
        "the harness turn should emit chat:updated so the UI refreshes"
    );
}

/// Through the app's send action: with a configured (but mock) provider, the
/// assistant reply is the model output, not the canned message.
#[test]
fn send_with_config_provider_appends_model_reply() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
    let (state, actions) = build(&desktop);

    // Configure a (mock) provider with a non-empty key so the send path treats
    // the model as connected.
    {
        let mut s = state.borrow_mut();
        s.selected_id = Some(chat_id.clone());
        s.settings_llm_provider = "mock".to_string();
        s.settings_llm_api_key = "test-key".to_string();
    }

    let rt = tokio::runtime::Runtime::new().expect("build runtime");
    let _guard = rt.enter();
    (actions.on_composer_change.borrow_mut())("Say hi".to_string());
    (actions.on_send_message.borrow_mut())("Say hi".to_string());
    wait_for_messages(&desktop, &chat_id, 2);

    let messages = desktop.list_chat_messages(&chat_id).expect("list messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].content, MOCK_REPLY);
    assert_ne!(messages[1].content, CANNED_REPLY);

    // The banner path is not triggered once a provider is configured.
    assert!(!state.borrow().show_llm_key_banner);
}

/// The composer's model dropdown is populated from the configured provider and
/// the selected model defaults to the configured model (so the label matches
/// what the send path will actually use).
#[test]
fn model_dropdown_populates_and_selects() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
    desktop
        .set_llm_setting("openai", "test-key", None, "gpt-4o-mini", None)
        .expect("save llm setting");

    let (state, actions) = build(&desktop);

    // Configured model is promoted to the front of the dropdown and selected.
    {
        let s = state.borrow();
        assert_eq!(s.settings_llm_provider, "openai");
        assert_eq!(
            s.models.first().map(String::as_str),
            Some("gpt-4o-mini"),
            "configured model should lead the dropdown"
        );
        assert!(s.models.iter().any(|m| m == "gpt-4o"), "catalog has more models");
        assert_eq!(s.selected_model, "gpt-4o-mini", "default selection is the configured model");
    }

    // Picking another item updates the composer selection and closes the menu.
    {
        *state.borrow_mut().model_menu_open.borrow_mut() = true;
    }
    (actions.on_model_select.borrow_mut())("gpt-4o".to_string());
    {
        let s = state.borrow();
        assert_eq!(s.selected_model, "gpt-4o");
        assert!(!*s.model_menu_open.borrow(), "menu should close after selecting");
    }

    // The selection survives a refresh (not overwritten back to the default).
    {
        let mut s = state.borrow_mut();
        s.selected_id = Some(chat_id.clone());
        s.refresh_settings(&desktop);
    }
    assert_eq!(state.borrow().selected_model, "gpt-4o");
}

/// Through the app's send action, the model that reaches the turn is the one
/// selected in the composer (falling back to the saved setting when empty).
/// With a mock provider this confirms the configured path still appends the
/// model reply (and not the canned string) once a composer model is picked.
#[test]
fn send_uses_composer_selected_model() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
    let (state, actions) = build(&desktop);

    {
        let mut s = state.borrow_mut();
        s.selected_id = Some(chat_id.clone());
        s.settings_llm_provider = "mock".to_string();
        s.settings_llm_api_key = "test-key".to_string();
        s.models = vec!["mock".to_string()];
        s.selected_model = "mock".to_string();
        s.refresh_settings(&desktop);
    }

    let rt = tokio::runtime::Runtime::new().expect("build runtime");
    let _guard = rt.enter();
    (actions.on_composer_change.borrow_mut())("Say hi".to_string());
    (actions.on_send_message.borrow_mut())("Say hi".to_string());
    wait_for_messages(&desktop, &chat_id, 2);

    let messages = desktop.list_chat_messages(&chat_id).expect("list messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].content, MOCK_REPLY);
    assert_ne!(messages[1].content, CANNED_REPLY);
    assert!(!state.borrow().show_llm_key_banner);
}

/// A configured send marks the agent busy (so the composer shows Stop) and
/// clearing it via Stop resets the flag. The mock turn runs on a background
/// task, so immediately after the send call the flag is still true.
#[test]
fn send_sets_agent_busy_and_stop_clears() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
    let (state, actions) = build(&desktop);

    {
        let mut s = state.borrow_mut();
        s.selected_id = Some(chat_id.clone());
        s.settings_llm_provider = "mock".to_string();
        s.settings_llm_api_key = "test-key".to_string();
        s.models = vec!["mock".to_string()];
        s.selected_model = "mock".to_string();
        s.refresh_settings(&desktop);
    }

    let rt = tokio::runtime::Runtime::new().expect("build runtime");
    let _guard = rt.enter();
    (actions.on_composer_change.borrow_mut())("Say hi".to_string());
    (actions.on_send_message.borrow_mut())("Say hi".to_string());
    assert!(
        state.borrow().agent_busy,
        "sending a configured turn should mark the agent busy"
    );

    (actions.on_stop.borrow_mut())();
    assert!(
        !state.borrow().agent_busy,
        "pressing Stop should clear the agent busy flag"
    );
}

/// The harness turn emits a `chat:turn_finished` event when it completes, and
/// its cancel slot is removed so `cancel_chat_turn` reports no running turn.
#[test]
fn run_chat_turn_emits_turn_finished() {
    use std::sync::Arc;
    use goble_desktop_service::CollectingEventBus;

    let (desktop, _dir) = common::desktop_state();
    let concrete = Arc::new(CollectingEventBus::new());
    desktop.set_event_bus(concrete.clone());
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");

    let rt = tokio::runtime::Runtime::new().expect("build runtime");
    let _guard = rt.enter();
    let handle = desktop
        .run_chat_turn(&chat_id, "Say hi", "mock", "")
        .expect("run turn");
    rt.block_on(handle).expect("task completes");

    assert!(
        concrete.has_event("chat:turn_finished"),
        "the harness turn should emit chat:turn_finished when it finishes"
    );

    // The turn has completed, so its cancel entry is removed: cancelling now
    // is a no-op that reports no turn in flight.
    assert!(
        !desktop.cancel_chat_turn(&chat_id),
        "cancel_chat_turn should report no running turn once the turn finished"
    );
}

/// `refresh_messages` maps persisted tool rows to a distinct `ChatRole::Tool`
/// (rendered as a terminal block) and surfaces the assistant message's tool-call
/// metadata, so tool invocations render distinctly instead of as assistant prose.
#[test]
fn refresh_messages_maps_tool_rows_and_evokes() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
    let now = "2026-08-25T00:00:00Z";

    let store = desktop.store_clone();
    store
        .insert_chat_message("m1", &chat_id, "user", "hello", None, now)
        .expect("insert user");
    store
        .insert_chat_message(
            "m2",
            &chat_id,
            "assistant",
            "Let me check.",
            Some(r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"}}]"#),
            now,
        )
        .expect("insert assistant with tool_calls");
    store
        .insert_chat_message("m3", &chat_id, "tool", "call_1\nfile.txt", None, now)
        .expect("insert tool result");

    let (state, _) = build(&desktop);
    {
        let mut s = state.borrow_mut();
        s.selected_id = Some(chat_id.clone());
        s.refresh_messages(&desktop);
    }

    let msgs = &state.borrow().chat_messages;
    assert_eq!(msgs.len(), 3, "user + assistant + tool rows");
    assert_eq!(msgs[0].role, ChatRole::User);
    assert_eq!(msgs[1].role, ChatRole::Assistant);
    assert_eq!(msgs[1].tool_calls.len(), 1, "assistant carries its tool-call metadata");
    assert_eq!(msgs[1].tool_calls[0].name, "ls");
    assert_eq!(
        msgs[2].role,
        ChatRole::Tool,
        "tool rows should render as Tool, not assistant prose"
    );
    assert!(
        matches!(msgs[2].fragments[0].kind, ChatFragmentKind::Terminal(_)),
        "tool output should be presented as a terminal block"
    );
}
