use super::injector::{build_context, should_compact, transcript_tail};
use goble_core::agent_memory::AgentMemory;
use goble_core::store::Store;

fn test_store() -> Store {
    Store::open_in_memory().unwrap()
}

fn seed_chat(store: &Store, chat_id: &str, count: usize) {
    store
        .insert_chat(
            chat_id,
            "Test",
            None,
            None,
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    for i in 0..count {
        let ts = format!("2024-01-01T00:{:02}:00Z", i);
        store
            .insert_chat_message(
                &format!("m{i}"),
                chat_id,
                if i % 2 == 0 { "user" } else { "assistant" },
                &format!("message {i}"),
                None,
                &ts,
            )
            .unwrap();
    }
}

#[test]
fn transcript_tail_keeps_only_most_recent_messages() {
    let store = test_store();
    seed_chat(&store, "c1", 10);
    let tail = transcript_tail(&store, "c1", 3).unwrap();
    assert!(tail.contains("message 7"));
    assert!(tail.contains("message 9"));
    assert!(!tail.contains("message 0"));
    assert!(!tail.contains("message 6"));
    // Rendered in chronological order.
    assert!(tail.find("message 7").unwrap() < tail.find("message 9").unwrap());
}

#[test]
fn transcript_tail_is_empty_for_missing_chat() {
    let store = test_store();
    let tail = transcript_tail(&store, "missing", 5).unwrap();
    assert!(tail.is_empty());
}

#[test]
fn should_compact_flips_at_threshold() {
    let store = test_store();
    seed_chat(&store, "c2", 5);
    assert!(!should_compact(&store, "c2", 5).unwrap());
    assert!(should_compact(&store, "c2", 4).unwrap());
}

#[test]
fn build_context_contains_identity_memory_and_tail() {
    let store = test_store();
    seed_chat(&store, "c3", 2);
    let memory = AgentMemory::new("a1", "ship v1");
    let tail = transcript_tail(&store, "c3", 5).unwrap();
    let prompt = build_context("you are a build agent", &memory, &tail);
    assert!(prompt.contains("you are a build agent"));
    assert!(prompt.contains("ship v1"));
    assert!(prompt.contains("message 0"));
}
