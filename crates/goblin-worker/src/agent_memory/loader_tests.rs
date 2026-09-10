use super::loader::{load_or_create, persist};
use goble_core::agent::AgentId;
use goble_core::store::Store;

fn test_store() -> Store {
    Store::open_in_memory().unwrap()
}

#[test]
fn load_or_create_seeds_from_spec_prompt_on_first_run() {
    let store = test_store();
    let agent_id = AgentId("agent-1".to_string());
    let memory = load_or_create(&store, &agent_id, "you are a build agent").unwrap();
    assert_eq!(memory.agent_id, "agent-1");
    assert_eq!(memory.brief, "you are a build agent");
    // Seeded memory is persisted so a second load is stable.
    let again = load_or_create(&store, &agent_id, "you are a build agent").unwrap();
    assert_eq!(again.agent_id, "agent-1");
    assert_eq!(again.version, memory.version);
}

#[test]
fn load_or_create_returns_existing_memory_without_reseeding() {
    let store = test_store();
    let agent_id = AgentId("agent-2".to_string());
    let mut memory = load_or_create(&store, &agent_id, "original brief").unwrap();
    memory.add_goal("keep goals across summarization");
    persist(&store, &memory).unwrap();

    // A later run with a different prompt must NOT overwrite persisted state.
    let loaded = load_or_create(&store, &agent_id, "a completely different prompt").unwrap();
    assert_eq!(loaded.brief, "original brief");
    assert_eq!(loaded.goals.len(), 1);
}

#[test]
fn persist_roundtrip_updates_version_and_content() {
    let store = test_store();
    let agent_id = AgentId("agent-3".to_string());
    let mut memory = load_or_create(&store, &agent_id, "brief").unwrap();
    memory.record_decision("use sqlite", "durable");
    persist(&store, &memory).unwrap();

    let loaded = store.get_agent_memory(&agent_id.0).unwrap().unwrap();
    assert_eq!(loaded.decisions.len(), 1);
    assert_eq!(loaded.decisions[0].summary, "use sqlite");
    assert!(loaded.updated_at >= memory.updated_at);
}
