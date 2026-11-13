use std::time::Duration;

use crate::subagent::SubAgentBudget;

/// The step budget a spawn gets when its call names none. A child that reads,
/// edits, runs a check and fixes what it found needs many turns; eight stopped
/// a real delegation before its first test run.
pub(crate) const DEFAULT_CHILD_STEPS: u32 = 200;

/// The largest step budget a caller may ask for: a mistyped number must not
/// turn a child into an unbounded run.
pub(crate) const MAX_CHILD_STEPS: u32 = 1_000;

/// The room a step gets for tokens. Every turn carries the conversation, so a
/// step costs well more than the text it adds.
const CHILD_TOKENS_PER_STEP: u64 = 10_000;

/// How long a foreground spawn waits for its child before the child moves to
/// the background and the call returns its id. The child is neither restarted
/// nor cancelled by this — its record stays live in the registry.
pub(crate) const DEFAULT_FOREGROUND_CHILD_WAIT: Duration = Duration::from_secs(600);

/// The budget a child gets for `steps` turns, or the default when the spawn
/// names none. Each step allows two tool calls and a prompt's worth of tokens,
/// so the turn ceiling is the one that normally stops a child.
pub(crate) fn child_budget(steps: Option<u32>) -> SubAgentBudget {
    let steps = steps
        .map(|steps| steps.clamp(1, MAX_CHILD_STEPS))
        .unwrap_or(DEFAULT_CHILD_STEPS);
    SubAgentBudget::new(
        steps,
        steps.saturating_mul(2),
        u64::from(steps) * CHILD_TOKENS_PER_STEP,
    )
}
