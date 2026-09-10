use super::context::ContextBuilder;
use super::memory::AgentMemory;

#[test]
fn test_build_composes_all_sections() {
    let mut memory = AgentMemory::new("agent-1", "the brief");
    memory.add_goal("goal one");
    let prompt = ContextBuilder::build(
        "You are an agent.",
        &memory,
        "user: hi\nassistant: hello",
    );
    assert!(prompt.contains("You are an agent."));
    assert!(prompt.contains("the brief"));
    assert!(prompt.contains("goal one"));
    assert!(prompt.contains("Recent conversation:"));
    assert!(prompt.contains("assistant: hello"));
}

#[test]
fn test_build_with_empty_tail() {
    let memory = AgentMemory::new("agent-1", "the brief");
    let prompt = ContextBuilder::build("You are an agent.", &memory, "");
    assert!(prompt.contains("the brief"));
    assert!(!prompt.contains("Recent conversation:"));
}
