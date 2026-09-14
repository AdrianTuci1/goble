//! The sub-agent model: what a spawned child runs from, its budget, its
//! status, and the record the transcript row and the work overlay read.
//!
//! Pure data and state machine — no I/O, no async, no store. The spawn tool
//! (S2), the registry and cancellation (S3) and the wire events (S4) build on
//! this module; the design is `.agents/04-agent-runtime/subagents.md`.

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Hard bound on the spawn tree. `depth` counts levels below the main agent,
/// which sits at depth 0, so a child of the main agent is at depth 1 and a
/// record at [`MAX_SUBAGENT_DEPTH`] may not spawn.
pub const MAX_SUBAGENT_DEPTH: u32 = 3;

/// The depth bound, as the predicate the spawn-tool builder calls. Past the
/// limit `spawn_subagent` is simply not in the child's tool set — the tool
/// going away is the enforcement, never a counter checked after the fact.
pub fn can_spawn_subagent(depth: u32) -> bool {
    depth < MAX_SUBAGENT_DEPTH
}

/// Directory under the workspace root that holds the per-child working dirs.
pub const SUBAGENTS_DIR: &str = "subagents";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubAgentId(pub String);

impl SubAgentId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// The per-child CWD rule: `workspace_root/subagents/<id>`. Derived from
    /// the id alone, so two children never share one directory. Pure path
    /// algebra — creating the directory on disk is the spawner's job (S2).
    pub fn child_cwd(&self, workspace_root: &Path) -> PathBuf {
        workspace_root.join(SUBAGENTS_DIR).join(&self.0)
    }
}

impl fmt::Display for SubAgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What the child runs from, as the spawn tool recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubAgentSpec {
    pub id: SubAgentId,
    /// The conversation that spawned the child. Chats are keyed by plain
    /// `String` ids in this tree (there is no `ChatId` newtype); the child's
    /// own conversation is keyed by `id`, never mixed into the parent's.
    pub parent_chat_id: String,
    /// The parent tool call that asked for it.
    pub parent_call_id: String,
    /// One line, shown in the transcript row and the work overlay.
    pub description: String,
    /// Resolves the child's prompt and tool set.
    pub subagent_type: String,
    pub prompt: String,
    /// A subdir of the workspace root, per [`SubAgentId::child_cwd`].
    pub cwd: PathBuf,
    /// `false`: the parent's tool call awaits the child and returns its
    /// output. `true`: the spawn returns the child's id at once.
    pub run_in_background: bool,
    /// Levels below the main agent (a child of it is at depth 1).
    pub depth: u32,
    pub budget: SubAgentBudget,
}

/// The per-child resource ceilings. Limits are hard: reaching one *is* the
/// exhaustion outcome, so "at the limit" is already out of budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubAgentBudget {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_tokens: u64,
}

impl SubAgentBudget {
    pub fn new(max_turns: u32, max_tool_calls: u32, tokens: u64) -> Self {
        Self {
            max_turns,
            max_tool_calls,
            max_tokens: tokens,
        }
    }

    /// Typed exhaustion predicate over usage so far: `Some` when any dimension
    /// has reached its limit, checked in the order turns, tool calls, tokens.
    /// The record's `charge_*` methods are the stateful form of this check.
    pub fn exhaustion(&self, turns: u32, tool_calls: u32, tokens: u64) -> Option<BudgetExhaustion> {
        if turns >= self.max_turns {
            Some(BudgetExhaustion::Turns {
                limit: self.max_turns,
            })
        } else if tool_calls >= self.max_tool_calls {
            Some(BudgetExhaustion::ToolCalls {
                limit: self.max_tool_calls,
            })
        } else if tokens >= self.max_tokens {
            Some(BudgetExhaustion::Tokens {
                limit: self.max_tokens,
            })
        } else {
            None
        }
    }
}

/// The typed reason a child ended on budget. Carrying the resource and the
/// limit is what lets the runner stop on a hard `Err` instead of looping on a
/// counter it has to re-check itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetExhaustion {
    Turns { limit: u32 },
    ToolCalls { limit: u32 },
    Tokens { limit: u64 },
}

impl BudgetExhaustion {
    /// The budget dimension that ran out, for wire payloads and logs.
    pub fn resource(&self) -> &'static str {
        match self {
            Self::Turns { .. } => "turns",
            Self::ToolCalls { .. } => "tool_calls",
            Self::Tokens { .. } => "tokens",
        }
    }
}

impl fmt::Display for BudgetExhaustion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Turns { limit } => {
                write!(f, "sub-agent budget exhausted: {limit} turns (max_turns)")
            }
            Self::ToolCalls { limit } => {
                write!(
                    f,
                    "sub-agent budget exhausted: {limit} tool calls (max_tool_calls)"
                )
            }
            Self::Tokens { limit } => {
                write!(f, "sub-agent budget exhausted: {limit} tokens (max_tokens)")
            }
        }
    }
}

impl std::error::Error for BudgetExhaustion {}

/// Where the child is. Terminal statuses are exactly `Completed`, `Failed` and
/// `Cancelled`; every other value means the child is still live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubAgentStatus {
    Initializing,
    Running {
        turns: u32,
        tool_calls: u32,
        tokens: u64,
        activity: String,
    },
    Completed {
        output: String,
        duration_ms: u64,
        turns: u32,
        tool_calls: u32,
        tokens: u64,
    },
    Failed {
        error: String,
    },
    Cancelled {
        reason: String,
    },
}

impl SubAgentStatus {
    /// Live: spawned but no terminal outcome yet.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Initializing | Self::Running { .. })
    }

    /// `Completed`, `Failed` or `Cancelled`. Exactly the complement of
    /// [`Self::is_running`], so the two predicates partition the enum.
    pub fn is_terminal(&self) -> bool {
        !self.is_running()
    }

    /// The counters the status carries; zeros while initializing or after a
    /// non-completing end.
    pub fn counts(&self) -> (u32, u32, u64) {
        match self {
            Self::Running {
                turns,
                tool_calls,
                tokens,
                ..
            }
            | Self::Completed {
                turns,
                tool_calls,
                tokens,
                ..
            } => (*turns, *tool_calls, *tokens),
            _ => (0, 0, 0),
        }
    }

    /// The one-line activity label the renderer prints on the transcript row:
    /// the counters plus the current activity while live, the outcome once
    /// terminal. Never contains a newline, whatever the strings hold.
    pub fn activity_label(&self) -> String {
        let label = match self {
            Self::Initializing => "initializing".to_string(),
            Self::Running { turns, tool_calls, tokens, activity } => {
                let base = format!("{turns} turns · {tool_calls} tool calls · {tokens} tokens");
                if activity.is_empty() {
                    base
                } else {
                    format!("{base} · {activity}")
                }
            }
            Self::Completed { duration_ms, turns, tool_calls, tokens, .. } => format!(
                "completed in {duration_ms} ms · {turns} turns · {tool_calls} tool calls · {tokens} tokens"
            ),
            Self::Failed { error } => format!("failed: {error}"),
            Self::Cancelled { reason } => format!("cancelled: {reason}"),
        };
        one_line(&label)
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// The live child, as the parent surfaces read it. `elapsed`/`duration` are
/// computed from the record's own timestamps, never from wall-clock guesses
/// about the status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubAgentRecord {
    pub spec: SubAgentSpec,
    pub status: SubAgentStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl SubAgentRecord {
    pub fn new(spec: SubAgentSpec) -> Self {
        Self {
            spec,
            status: SubAgentStatus::Initializing,
            started_at: Utc::now(),
            finished_at: None,
        }
    }

    /// `Initializing` → `Running` at zero counters. No-op once live or terminal.
    pub fn begin_running(&mut self) {
        if self.status == SubAgentStatus::Initializing {
            self.status = SubAgentStatus::Running {
                turns: 0,
                tool_calls: 0,
                tokens: 0,
                activity: String::new(),
            };
        }
    }

    /// Update the in-flight activity line. No-op unless the child is `Running`.
    pub fn set_activity(&mut self, activity: impl Into<String>) {
        if let SubAgentStatus::Running { activity: a, .. } = &mut self.status {
            *a = activity.into();
        }
    }

    /// Charge one model turn. `Err` means the budget is exhausted and the
    /// record is *already terminal* (`Failed`, typed reason in the error) —
    /// the runner stops on the `Err`, it does not need to keep watching a
    /// counter. Charges on an already-terminal record are no-ops that keep the
    /// first terminal outcome. `Initializing` self-promotes to `Running`.
    pub fn charge_turn(&mut self) -> Result<(), BudgetExhaustion> {
        self.charge(1, 0, 0)
    }

    /// Charge one tool call; same terminal contract as [`Self::charge_turn`].
    pub fn charge_tool_call(&mut self) -> Result<(), BudgetExhaustion> {
        self.charge(0, 1, 0)
    }

    /// Charge `added` tokens; same terminal contract as [`Self::charge_turn`].
    pub fn charge_tokens(&mut self, added: u64) -> Result<(), BudgetExhaustion> {
        self.charge(0, 0, added)
    }

    fn charge(&mut self, turns: u32, tool_calls: u32, tokens: u64) -> Result<(), BudgetExhaustion> {
        if self.status.is_terminal() {
            return Ok(());
        }
        self.begin_running();
        if let SubAgentStatus::Running {
            turns: ct,
            tool_calls: cc,
            tokens: ctk,
            ..
        } = &mut self.status
        {
            *ct += turns;
            *cc += tool_calls;
            *ctk += tokens;
        }
        let (ct, cc, ctk) = self.status.counts();
        match self.spec.budget.exhaustion(ct, cc, ctk) {
            Some(exhaustion) => {
                // Terminal by construction: the exhaustion ends the child here,
                // exactly as if a tool had errored.
                self.fail(exhaustion.to_string());
                Err(exhaustion)
            }
            None => Ok(()),
        }
    }

    /// The child's own conversation is its `spec.id` — the chat key the child
    /// view and the store (S2) read messages under.
    pub fn chat_id(&self) -> &str {
        &self.spec.id.0
    }

    /// `Running` → `Completed`, carrying the counters charged so far and the
    /// duration from the record's own clock. First terminal outcome wins.
    pub fn complete(&mut self, output: String) {
        if self.status.is_terminal() {
            return;
        }
        let now = Utc::now();
        let (turns, tool_calls, tokens) = self.status.counts();
        let duration_ms = ms_between(self.started_at, now);
        self.status = SubAgentStatus::Completed {
            output,
            duration_ms,
            turns,
            tool_calls,
            tokens,
        };
        self.finished_at = Some(now);
    }

    /// `Running` → `Failed`, the child errored (budget or otherwise).
    /// First terminal outcome wins.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.finish_terminal(|error| SubAgentStatus::Failed { error }, error.into());
    }

    /// `Running` → `Cancelled`, from the parent's cancel bit or an overlay kill.
    /// First terminal outcome wins.
    pub fn cancel(&mut self, reason: impl Into<String>) {
        self.finish_terminal(|reason| SubAgentStatus::Cancelled { reason }, reason.into());
    }

    fn finish_terminal(&mut self, make: impl FnOnce(String) -> SubAgentStatus, text: String) {
        if self.status.is_terminal() {
            return;
        }
        self.status = make(text);
        self.finished_at = Some(Utc::now());
    }

    /// Milliseconds spent: `started_at` → `finished_at` once terminal, else →
    /// `now`. From the record's own timestamps only.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms_at(Utc::now())
    }

    /// Testable form of [`Self::elapsed_ms`] with the clock passed in.
    pub fn elapsed_ms_at(&self, now: DateTime<Utc>) -> u64 {
        ms_between(self.started_at, self.finished_at.unwrap_or(now))
    }

    /// `Some` once a terminal outcome has stamped `finished_at`.
    pub fn duration_ms(&self) -> Option<u64> {
        self.finished_at
            .map(|finished| ms_between(self.started_at, finished))
    }
}

fn ms_between(start: DateTime<Utc>, end: DateTime<Utc>) -> u64 {
    (end - start).num_milliseconds().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::path::Path;

    fn spec_with(budget: SubAgentBudget) -> SubAgentSpec {
        SubAgentSpec {
            id: SubAgentId("child-1".into()),
            parent_chat_id: "parent-chat".into(),
            parent_call_id: "call-9".into(),
            description: "review the diff".into(),
            subagent_type: "reviewer".into(),
            prompt: "review it".into(),
            cwd: Path::new("/work/subagents/child-1").to_path_buf(),
            run_in_background: false,
            depth: 1,
            budget,
        }
    }

    fn record_with(budget: SubAgentBudget) -> SubAgentRecord {
        SubAgentRecord::new(spec_with(budget))
    }

    #[test]
    fn turns_budget_is_terminal_at_its_limit() {
        let mut rec = record_with(SubAgentBudget::new(2, 100, 1_000_000));
        assert!(rec.charge_turn().is_ok());
        assert!(rec.status.is_running());
        let err = rec
            .charge_turn()
            .expect_err("reaching max_turns is exhaustion");
        assert_eq!(err, BudgetExhaustion::Turns { limit: 2 });
        assert_eq!(err.resource(), "turns");
        assert!(rec.status.is_terminal() && !rec.status.is_running());
        match &rec.status {
            SubAgentStatus::Failed { error } => {
                assert!(
                    error.contains("exhausted") && error.contains("turns"),
                    "{error}"
                );
            }
            other => panic!("exhaustion must end the child Failed, got {other:?}"),
        }
        assert!(
            rec.finished_at.is_some(),
            "a terminal outcome stamps the record"
        );
    }

    #[test]
    fn tool_calls_budget_is_terminal_at_its_limit() {
        let mut rec = record_with(SubAgentBudget::new(100, 1, 1_000_000));
        let err = rec
            .charge_tool_call()
            .expect_err("reaching max_tool_calls is exhaustion");
        assert_eq!(err, BudgetExhaustion::ToolCalls { limit: 1 });
        assert!(rec.status.is_terminal());
        assert!(
            matches!(&rec.status, SubAgentStatus::Failed { error } if error.contains("tool_calls"))
        );
    }

    #[test]
    fn tokens_budget_is_terminal_at_its_limit() {
        let mut rec = record_with(SubAgentBudget::new(100, 100, 100));
        assert!(rec.charge_tokens(60).is_ok());
        let err = rec
            .charge_tokens(50)
            .expect_err("passing max_tokens is exhaustion");
        assert_eq!(err, BudgetExhaustion::Tokens { limit: 100 });
        assert!(rec.status.is_terminal());
        assert!(
            matches!(&rec.status, SubAgentStatus::Failed { error } if error.contains("tokens"))
        );
    }

    #[test]
    fn exhaustion_ends_the_child_once_and_the_run_never_hangs() {
        // After the first Err the record is terminal, so a runner that only
        // checks liveness stops: further charges are no-ops, not new errors
        // and not status churn.
        let mut rec = record_with(SubAgentBudget::new(1, 1, 1));
        assert!(rec.charge_turn().is_err());
        let first = rec.status.clone();
        assert!(rec.charge_turn().is_ok());
        assert!(rec.charge_tool_call().is_ok());
        assert!(rec.charge_tokens(10).is_ok());
        assert_eq!(rec.status, first);
        assert!(!rec.status.is_running());
    }

    #[test]
    fn charging_below_every_limit_keeps_the_child_running() {
        let mut rec = record_with(SubAgentBudget::new(3, 5, 1_000));
        for _ in 0..2 {
            assert!(rec.charge_turn().is_ok());
        }
        for _ in 0..4 {
            assert!(rec.charge_tool_call().is_ok());
        }
        assert!(rec.charge_tokens(999).is_ok());
        assert!(rec.status.is_running());
        assert_eq!(rec.status.counts(), (2, 4, 999));
        // The first charge promoted Initializing to Running.
        assert!(matches!(rec.status, SubAgentStatus::Running { .. }));
    }

    #[test]
    fn budget_check_is_pure_and_orders_turns_calls_tokens() {
        let budget = SubAgentBudget::new(2, 2, 2);
        assert_eq!(budget.exhaustion(1, 1, 1), None);
        assert_eq!(
            budget.exhaustion(2, 2, 2),
            Some(BudgetExhaustion::Turns { limit: 2 })
        );
        assert_eq!(
            budget.exhaustion(1, 2, 9),
            Some(BudgetExhaustion::ToolCalls { limit: 2 })
        );
        assert_eq!(
            budget.exhaustion(1, 1, 3),
            Some(BudgetExhaustion::Tokens { limit: 2 })
        );
    }

    #[test]
    fn depth_predicate_refuses_at_the_limit_and_allows_below() {
        for depth in 0..MAX_SUBAGENT_DEPTH {
            assert!(
                can_spawn_subagent(depth),
                "depth {depth} is below the limit"
            );
        }
        assert!(
            !can_spawn_subagent(MAX_SUBAGENT_DEPTH),
            "the spawn tool is gone at the limit"
        );
        assert!(!can_spawn_subagent(MAX_SUBAGENT_DEPTH + 1));
        assert!(!can_spawn_subagent(u32::MAX));
    }

    #[test]
    fn child_cwd_is_a_per_id_subdir_of_the_workspace_root() {
        let root = Path::new("/workspaces/acme");
        let a = SubAgentId::generate();
        let b = SubAgentId::generate();
        assert_ne!(a, b);
        let ca = a.child_cwd(root);
        let cb = b.child_cwd(root);
        assert_ne!(ca, cb, "two children must never share a working directory");
        for (id, cwd) in [(&a, &ca), (&b, &cb)] {
            assert!(
                cwd.starts_with(root.join(SUBAGENTS_DIR)),
                "{cwd:?} escapes the root"
            );
            assert_eq!(
                cwd.file_name().and_then(|s| s.to_str()),
                Some(id.0.as_str())
            );
        }
        // Deterministic in the id: the same id always maps back to the same dir.
        assert_eq!(ca, a.child_cwd(root));
    }

    #[test]
    fn elapsed_and_duration_come_from_the_records_own_timestamps() {
        let started = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
        let finished = started + chrono::TimeDelta::milliseconds(2500);

        let mut live = SubAgentRecord {
            spec: spec_with(SubAgentBudget::new(10, 10, 10)),
            status: SubAgentStatus::Initializing,
            started_at: started,
            finished_at: None,
        };
        assert_eq!(
            live.elapsed_ms_at(started + chrono::TimeDelta::milliseconds(900)),
            900
        );
        assert_eq!(
            live.duration_ms(),
            None,
            "no duration before a terminal outcome"
        );

        live.finished_at = Some(finished);
        // Wall-clock `now` is far past the run; the numbers must still come
        // from the record, not from it.
        assert_eq!(live.elapsed_ms(), 2500);
        assert_eq!(live.duration_ms(), Some(2500));

        // The record computes duration from timestamps even when the status
        // carries a different (wire-supplied) figure.
        live.status = SubAgentStatus::Completed {
            output: "done".into(),
            duration_ms: 1,
            turns: 3,
            tool_calls: 4,
            tokens: 5,
        };
        assert_eq!(live.duration_ms(), Some(2500));
    }

    #[test]
    fn complete_keeps_counters_and_stamps_the_duration() {
        let mut rec = record_with(SubAgentBudget::new(10, 10, 10));
        assert!(rec.charge_turn().is_ok());
        assert!(rec.charge_tool_call().is_ok());
        assert!(rec.charge_tokens(7).is_ok());
        rec.complete("all good".into());
        match &rec.status {
            SubAgentStatus::Completed {
                output,
                turns,
                tool_calls,
                tokens,
                duration_ms,
            } => {
                assert_eq!(output, "all good");
                assert_eq!((*turns, *tool_calls, *tokens), (1, 1, 7));
                assert!(
                    *duration_ms < 5_000,
                    "duration stamped at completion: {duration_ms}"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
        assert!(rec.finished_at.is_some());
        assert_eq!(rec.chat_id(), "child-1");
    }

    #[test]
    fn first_terminal_outcome_wins() {
        let mut rec = record_with(SubAgentBudget::new(10, 10, 10));
        rec.begin_running();
        rec.cancel("killed from the overlay");
        rec.fail("late error");
        rec.complete("late output".into());
        assert!(
            matches!(&rec.status, SubAgentStatus::Cancelled { reason } if reason == "killed from the overlay")
        );
    }

    #[test]
    fn every_variant_distinguishes_running_from_terminal() {
        let statuses = vec![
            (SubAgentStatus::Initializing, false),
            (
                SubAgentStatus::Running {
                    turns: 1,
                    tool_calls: 1,
                    tokens: 1,
                    activity: "x".into(),
                },
                false,
            ),
            (
                SubAgentStatus::Completed {
                    output: "o".into(),
                    duration_ms: 5,
                    turns: 1,
                    tool_calls: 1,
                    tokens: 1,
                },
                true,
            ),
            (
                SubAgentStatus::Failed {
                    error: "boom".into(),
                },
                true,
            ),
            (
                SubAgentStatus::Cancelled {
                    reason: "stop".into(),
                },
                true,
            ),
        ];
        for (status, terminal) in statuses {
            assert_eq!(status.is_terminal(), terminal, "{status:?}");
            assert_eq!(status.is_running(), !terminal, "{status:?}");
            assert_ne!(
                status.is_running(),
                status.is_terminal(),
                "the two partition the enum"
            );
        }
    }

    #[test]
    fn activity_label_is_one_line_per_variant() {
        let running = SubAgentStatus::Running {
            turns: 2,
            tool_calls: 3,
            tokens: 1200,
            activity: "reading crates/goble-core/src/harness.rs".into(),
        };
        let label = running.activity_label();
        assert!(
            label.contains("reading crates/goble-core/src/harness.rs"),
            "{label}"
        );
        assert!(label.contains('2') && label.contains("1200"), "{label}");
        assert!(!label.contains('\n'));

        // Newlines inside hostile strings cannot break the single-line rule.
        let nasty = SubAgentStatus::Running {
            turns: 0,
            tool_calls: 0,
            tokens: 0,
            activity: "a\nb\r\nc".into(),
        };
        let label = nasty.activity_label();
        assert!(!label.contains('\n') && !label.contains('\r'), "{label}");

        let labels = vec![
            SubAgentStatus::Initializing.activity_label(),
            running.activity_label(),
            SubAgentStatus::Completed {
                output: "o".into(),
                duration_ms: 1500,
                turns: 1,
                tool_calls: 1,
                tokens: 1,
            }
            .activity_label(),
            SubAgentStatus::Failed {
                error: "boom".into(),
            }
            .activity_label(),
            SubAgentStatus::Cancelled {
                reason: "user stop".into(),
            }
            .activity_label(),
        ];
        for label in labels {
            assert!(!label.is_empty() && !label.contains('\n'), "{label:?}");
        }
        assert!(SubAgentStatus::Cancelled {
            reason: "user stop".into()
        }
        .activity_label()
        .contains("user stop"));
    }

    #[test]
    fn record_roundtrips_through_serde() {
        // S4 payloads mirror these fields; the tagged enum shape is the
        // convention every other harness model uses.
        let mut rec = record_with(SubAgentBudget::new(4, 6, 800));
        rec.begin_running();
        rec.set_activity("searching");
        let json = serde_json::to_string(&rec).unwrap();
        let back: SubAgentRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
        assert!(json.contains("\"kind\":\"running\""), "{json}");
        assert!(json.contains("searching"));
    }
}
