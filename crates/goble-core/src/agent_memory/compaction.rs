use serde::{Deserialize, Serialize};

use super::memory::AgentMemory;

/// Structured output produced by a compaction turn. The model fills this
/// schema instead of writing free-form prose, so the merge step is mechanical
/// and no requirement can be silently lost by the summarizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionResult {
    pub session_id: String,
    pub summary: String,
    pub facts: Vec<String>,
    pub decisions: Vec<CompactedDecision>,
    pub goals_completed: Vec<String>,
    pub new_goals: Vec<String>,
    pub milestones_completed: Vec<String>,
    pub next_steps: Vec<String>,
    pub message_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactedDecision {
    pub summary: String,
    pub rationale: String,
}

/// Prompt used to compact an overflowing transcript into [`CompactionResult`].
pub const COMPACTION_PROMPT: &str = r#"You are compacting an agent conversation. Read the conversation and produce a JSON object matching this schema:
{
  "session_id": "stable id for this session",
  "summary": "short paragraph capturing what happened",
  "facts": ["facts learned about the project/user that must be remembered"],
  "decisions": [{"summary": "the decision", "rationale": "why it was made"}],
  "goals_completed": ["goal texts that are now complete"],
  "new_goals": ["new goal texts that emerged"],
  "milestones_completed": ["milestone texts now complete"],
  "next_steps": ["concrete next actions"],
  "message_count": number of messages in the conversation
}
Return ONLY the JSON object."#;

/// Merge a compaction result into the agent's canonical memory.
/// Facts/decisions/goals are deduplicated; summaries are appended, never replaced.
pub fn merge_compaction(memory: &mut AgentMemory, result: CompactionResult) {
    memory.append_summary(result.session_id, result.summary, result.message_count);

    for fact in result.facts {
        memory.add_fact(fact);
    }

    for decision in result.decisions {
        let exists = memory
            .decisions
            .iter()
            .any(|d| d.summary == decision.summary);
        if !exists {
            memory.record_decision(decision.summary, decision.rationale);
        }
    }

    for text in result.goals_completed {
        if let Some(goal) = memory
            .goals
            .iter_mut()
            .find(|g| text_matches(&g.text, &text))
        {
            goal.done = true;
        }
    }
    for text in result.new_goals {
        let exists = memory.goals.iter().any(|g| text_matches(&g.text, &text));
        if !exists {
            memory.add_goal(text);
        }
    }

    for text in result.milestones_completed {
        if let Some(m) = memory
            .progress
            .iter_mut()
            .find(|m| text_matches(&m.text, &text))
        {
            m.done = true;
        } else {
            let id = memory.add_milestone(text.clone());
            if let Some(m) = memory.progress.iter_mut().find(|m| m.id == id) {
                m.done = true;
            }
        }
    }

    for text in result.next_steps {
        let exists = memory
            .progress
            .iter()
            .any(|m| text_matches(&m.text, &text));
        if !exists {
            memory.add_milestone(text);
        }
    }
}

/// Fuzzy equality used to deduplicate goal/milestone texts across compactions.
fn text_matches(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    a == b || a.contains(&b) || b.contains(&a)
}
