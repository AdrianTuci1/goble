use super::compaction::*;
use super::memory::AgentMemory;

#[test]
fn test_merge_adds_facts_and_decisions() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    merge_compaction(
        &mut memory,
        CompactionResult {
            session_id: "s1".into(),
            summary: "did things".into(),
            facts: vec!["user prefers blue".into()],
            decisions: vec![CompactedDecision {
                summary: "use sqlite".into(),
                rationale: "simple".into(),
            }],
            goals_completed: vec![],
            new_goals: vec![],
            milestones_completed: vec![],
            next_steps: vec![],
            message_count: 12,
        },
    );
    assert_eq!(memory.facts, vec!["user prefers blue".to_string()]);
    assert_eq!(memory.decisions.len(), 1);
    assert_eq!(memory.rolling_summaries.len(), 1);
    assert_eq!(memory.rolling_summaries[0].message_count, 12);
}

#[test]
fn test_merge_dedupes_on_second_run() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    let result = || CompactionResult {
        session_id: "s1".into(),
        summary: "did things".into(),
        facts: vec!["fact".into()],
        decisions: vec![CompactedDecision {
            summary: "decision".into(),
            rationale: "why".into(),
        }],
        goals_completed: vec![],
        new_goals: vec!["goal".into()],
        milestones_completed: vec![],
        next_steps: vec![],
        message_count: 12,
    };
    merge_compaction(&mut memory, result());
    merge_compaction(&mut memory, result());
    assert_eq!(memory.facts.len(), 1);
    assert_eq!(memory.decisions.len(), 1);
    assert_eq!(memory.goals.len(), 1);
    assert_eq!(memory.rolling_summaries.len(), 2);
}

#[test]
fn test_merge_completes_goals_and_milestones() {
    let mut memory = AgentMemory::new("agent-1", "brief");
    memory.add_goal("ship v1");
    merge_compaction(
        &mut memory,
        CompactionResult {
            session_id: "s1".into(),
            summary: "done".into(),
            facts: vec![],
            decisions: vec![],
            goals_completed: vec!["ship v1".into()],
            new_goals: vec![],
            milestones_completed: vec!["add tests".into()],
            next_steps: vec!["write docs".into()],
            message_count: 5,
        },
    );
    assert!(memory.goals[0].done);
    assert!(
        memory
            .progress
            .iter()
            .any(|m| m.text == "add tests" && m.done)
    );
    assert!(
        memory
            .progress
            .iter()
            .any(|m| m.text == "write docs" && !m.done)
    );
}
