use super::memory::*;

#[test]
fn test_new_initializes_brief() {
    let memory = AgentMemory::new("agent-1", "build the thing");
    assert_eq!(memory.agent_id, "agent-1");
    assert_eq!(memory.brief, "build the thing");
    assert_eq!(memory.version, AGENT_MEMORY_VERSION);
}

#[test]
fn test_version_bumps_on_mutation() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    let v0 = memory.version;
    memory.add_goal("goal");
    assert!(memory.version > v0);
    let v1 = memory.version;
    memory.add_fact("fact");
    assert!(memory.version > v1);
}

#[test]
fn test_goal_lifecycle() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    let id = memory.add_goal("ship v1");
    assert!(!memory.goals[0].done);
    assert!(memory.complete_goal(&id));
    assert!(memory.goals[0].done);
    assert!(!memory.complete_goal("missing"));
}

#[test]
fn test_dedup_facts_and_constraints() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    memory.add_fact("a");
    memory.add_fact("a");
    assert_eq!(memory.facts.len(), 1);
    memory.add_constraint("c");
    memory.add_constraint("c");
    assert_eq!(memory.constraints.len(), 1);
}

#[test]
fn test_decision_and_milestone() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    let d = memory.record_decision("use rust", "performance");
    assert_eq!(memory.decisions.len(), 1);
    assert_eq!(memory.decisions[0].id, d);
    let m = memory.add_milestone("write tests");
    assert!(!memory.progress[0].done);
    assert!(memory.complete_milestone(&m));
    assert!(memory.progress[0].done);
}

#[test]
fn test_summaries_append_and_render() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    memory.append_summary("s1", "first", 10);
    memory.append_summary("s2", "second", 20);
    assert_eq!(memory.rolling_summaries.len(), 2);
    let block = memory.render_block();
    assert!(block.contains("Brief: brief"));
    assert!(block.contains("second"));
    assert!(block.contains("Agent memory (v"));
}

#[test]
fn test_open_questions() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    memory.add_open_question("which db?");
    memory.add_open_question("which db?");
    assert_eq!(memory.open_questions.len(), 1);
    assert!(memory.resolve_open_question("which db?"));
    assert!(memory.open_questions.is_empty());
}
