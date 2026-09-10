use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Current schema version for [`AgentMemory`].
pub const AGENT_MEMORY_VERSION: u32 = 1;

/// An active or completed goal. Goals come from the user's requirements and
/// survive transcript compaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub created_at: DateTime<Utc>,
}

impl Goal {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.into(),
            done: false,
            created_at: Utc::now(),
        }
    }
}

/// A recorded decision with rationale, ADR-style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub summary: String,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}

impl Decision {
    pub fn new(summary: impl Into<String>, rationale: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            summary: summary.into(),
            rationale: rationale.into(),
            created_at: Utc::now(),
        }
    }
}

/// A milestone in the agent's progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub created_at: DateTime<Utc>,
}

impl Milestone {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.into(),
            done: false,
            created_at: Utc::now(),
        }
    }
}

/// A rolling summary produced when a conversation window is compacted.
/// Appended, never replaced, so the agent keeps a coarse history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub summary: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
}

impl SessionSummary {
    pub fn new(
        session_id: impl Into<String>,
        summary: impl Into<String>,
        message_count: usize,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            summary: summary.into(),
            message_count,
            created_at: Utc::now(),
        }
    }
}

/// Canonical per-agent memory. This is the source of truth that survives
/// conversation summarization: transcripts may be compacted freely, but this
/// state only changes through explicit writes (user edits or agent tools).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMemory {
    pub agent_id: String,
    pub version: u32,
    pub brief: String,
    pub facts: Vec<String>,
    pub goals: Vec<Goal>,
    pub constraints: Vec<String>,
    pub decisions: Vec<Decision>,
    pub progress: Vec<Milestone>,
    pub open_questions: Vec<String>,
    pub rolling_summaries: Vec<SessionSummary>,
    pub updated_at: DateTime<Utc>,
}

impl AgentMemory {
    pub fn new(agent_id: impl Into<String>, brief: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            version: AGENT_MEMORY_VERSION,
            brief: brief.into(),
            facts: Vec::new(),
            goals: Vec::new(),
            constraints: Vec::new(),
            decisions: Vec::new(),
            progress: Vec::new(),
            open_questions: Vec::new(),
            rolling_summaries: Vec::new(),
            updated_at: Utc::now(),
        }
    }

    /// Bump the version and refresh `updated_at` after any mutation.
    pub fn bump_version(&mut self) {
        self.version += 1;
        self.updated_at = Utc::now();
    }

    pub fn update_brief(&mut self, brief: impl Into<String>) {
        self.brief = brief.into();
        self.bump_version();
    }

    pub fn add_fact(&mut self, fact: impl Into<String>) {
        let fact = fact.into();
        if !self.facts.iter().any(|f| f == &fact) {
            self.facts.push(fact);
            self.bump_version();
        }
    }

    pub fn add_goal(&mut self, text: impl Into<String>) -> String {
        let goal = Goal::new(text);
        let id = goal.id.clone();
        self.goals.push(goal);
        self.bump_version();
        id
    }

    pub fn complete_goal(&mut self, id: &str) -> bool {
        if let Some(goal) = self.goals.iter_mut().find(|g| g.id == id) {
            if !goal.done {
                goal.done = true;
                self.bump_version();
            }
            true
        } else {
            false
        }
    }

    pub fn add_constraint(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.constraints.iter().any(|c| c == &text) {
            self.constraints.push(text);
            self.bump_version();
        }
    }

    pub fn record_decision(&mut self, summary: impl Into<String>, rationale: impl Into<String>) -> String {
        let decision = Decision::new(summary, rationale);
        let id = decision.id.clone();
        self.decisions.push(decision);
        self.bump_version();
        id
    }

    pub fn add_milestone(&mut self, text: impl Into<String>) -> String {
        let milestone = Milestone::new(text);
        let id = milestone.id.clone();
        self.progress.push(milestone);
        self.bump_version();
        id
    }

    pub fn complete_milestone(&mut self, id: &str) -> bool {
        if let Some(m) = self.progress.iter_mut().find(|m| m.id == id) {
            if !m.done {
                m.done = true;
                self.bump_version();
            }
            true
        } else {
            false
        }
    }

    pub fn add_open_question(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.open_questions.iter().any(|q| q == &text) {
            self.open_questions.push(text);
            self.bump_version();
        }
    }

    pub fn resolve_open_question(&mut self, text: &str) -> bool {
        let before = self.open_questions.len();
        self.open_questions.retain(|q| q != text);
        if self.open_questions.len() != before {
            self.bump_version();
            true
        } else {
            false
        }
    }

    pub fn append_summary(
        &mut self,
        session_id: impl Into<String>,
        summary: impl Into<String>,
        message_count: usize,
    ) {
        self.rolling_summaries
            .push(SessionSummary::new(session_id, summary, message_count));
        self.bump_version();
    }

    /// Render the memory as a compact structured text block for prompt injection.
    pub fn render_block(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Agent memory (v{})\n", self.version));
        out.push_str(&format!("Brief: {}\n", self.brief));
        if !self.facts.is_empty() {
            out.push_str("Facts:\n");
            for f in &self.facts {
                out.push_str(&format!("- {f}\n"));
            }
        }
        if !self.goals.is_empty() {
            out.push_str("Goals:\n");
            for g in &self.goals {
                let mark = if g.done { "[x]" } else { "[ ]" };
                out.push_str(&format!("- {mark} {}\n", g.text));
            }
        }
        if !self.constraints.is_empty() {
            out.push_str("Constraints:\n");
            for c in &self.constraints {
                out.push_str(&format!("- {c}\n"));
            }
        }
        if !self.decisions.is_empty() {
            out.push_str("Decisions:\n");
            for d in &self.decisions {
                out.push_str(&format!("- {} ({})\n", d.summary, d.rationale));
            }
        }
        if !self.progress.is_empty() {
            out.push_str("Progress:\n");
            for m in &self.progress {
                let mark = if m.done { "[x]" } else { "[ ]" };
                out.push_str(&format!("- {mark} {}\n", m.text));
            }
        }
        if !self.open_questions.is_empty() {
            out.push_str("Open questions:\n");
            for q in &self.open_questions {
                out.push_str(&format!("- {q}\n"));
            }
        }
        if !self.rolling_summaries.is_empty() {
            out.push_str("Session summaries (newest first):\n");
            for s in self.rolling_summaries.iter().rev().take(5) {
                out.push_str(&format!("- [{} msgs] {}\n", s.message_count, s.summary));
            }
        }
        out
    }
}
