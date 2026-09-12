//! Unit tests for the desktop service's state, grouped by the surface they cover.

use std::sync::Arc;

use goble_core::store::Store;

use super::*;

mod service;
mod turns;

fn tmp_state() -> (tempfile::TempDir, Arc<DesktopState>) {
    let dir = tempfile::tempdir().unwrap();
    let state = DesktopState::new(
        Store::open_in_memory().unwrap(),
        crate::thread_store::ThreadStore::new(dir.path()).unwrap(),
    );
    (dir, state)
}

/// A scripted internal harness whose first execution step plans one
/// `run_command` call, so the turn suspends on the approval gate. The
/// default runner is the harness's `MockCommandRunner`; pass a runner to
/// exercise a different outcome (e.g. an approved command that fails).
fn scripted_command_harness_with_runner(
    store: Store,
    runner: Option<Arc<dyn goble_core::harness::CommandRunner>>,
) -> Arc<goble_harness_internal::InternalHarness> {
    use goble_core::llm::{CompletionResponse, LlmToolCall, MockProvider};
    let provider = MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "call-1".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "echo", "args": ["hi"]}),
            }],
            usage: None,
        },
    );
    let mut harness = goble_harness_internal::InternalHarness::new(store)
        .with_id(goble_harness_types::HarnessId::new("scripted"))
        .with_provider("mock")
        .with_model("mock")
        .with_llm(Arc::new(provider));
    if let Some(runner) = runner {
        harness = harness.with_runner(runner);
    }
    Arc::new(harness)
}

fn wait_for_bus_event(bus: &crate::event_bus::CollectingEventBus, event: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !bus.has_event(event) {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {event}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Run a command turn up to its approval suspension, returning the state,
/// the event bus, the chat id and the shared store. Callers hold an entered
/// runtime so the daemon tasks stay alive across the assertions.
fn command_suspension() -> (
    tempfile::TempDir,
    Arc<DesktopState>,
    Arc<crate::event_bus::CollectingEventBus>,
    String,
    Store,
) {
    command_suspension_with_runner(None)
}

fn command_suspension_with_runner(
    runner: Option<Arc<dyn goble_core::harness::CommandRunner>>,
) -> (
    tempfile::TempDir,
    Arc<DesktopState>,
    Arc<crate::event_bus::CollectingEventBus>,
    String,
    Store,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let state = DesktopState::new(
        store.clone(),
        crate::thread_store::ThreadStore::new(dir.path()).unwrap(),
    );
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let chat_id = state.create_chat("Demo", None, None).expect("create chat");
    state.register_harness(scripted_command_harness_with_runner(store.clone(), runner));

    let handle = state
        .run_chat_turn(
            &chat_id,
            "run echo hi",
            "mock",
            "",
            "local",
            "default",
            &chat_id,
            None,
            Some("scripted"),
        )
        .expect("run turn on the scripted harness");
    // The turn suspends on the proposal, so its TraceFinished never arrives
    // and the handle stays pending; drop it and wait on the wire event.
    drop(handle);
    wait_for_bus_event(&bus, "chat:command_proposed");
    assert!(
        !bus.has_event("chat:turn_finished"),
        "a suspended command must not finish the turn"
    );
    (dir, state, bus, chat_id, store)
}

fn tool_messages(store: &Store, chat_id: &str) -> Vec<String> {
    store
        .list_chat_messages(chat_id)
        .unwrap()
        .into_iter()
        .filter(|m| m.1 == "tool")
        .map(|m| m.2)
        .collect()
}
