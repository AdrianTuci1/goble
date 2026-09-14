use std::time::Duration;

use crate::subagent::SubAgentBudget;

/// The child's ceilings, as the spawn handler stamps them into the spec.
/// Charges are hard: reaching a limit *is* the exhaustion outcome, so the
/// child gets strictly fewer actions than the limit (S1's budget contract).
const CHILD_MAX_TURNS: u32 = 8;
const CHILD_MAX_TOOL_CALLS: u32 = 16;
const CHILD_MAX_TOKENS: u64 = 100_000;

/// How long a foreground spawn waits for its child before the child moves to
/// the background and the call returns its id. The child is neither restarted
/// nor cancelled by this — its record stays live in the registry.
pub(crate) const DEFAULT_FOREGROUND_CHILD_WAIT: Duration = Duration::from_secs(600);

pub(crate) fn default_child_budget() -> SubAgentBudget {
    SubAgentBudget::new(CHILD_MAX_TURNS, CHILD_MAX_TOOL_CALLS, CHILD_MAX_TOKENS)
}
