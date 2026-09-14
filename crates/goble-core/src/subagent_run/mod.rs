//! The child run: its bounded turn loop, the registry its record lives in and
//! the cancellation that stops it (S2 + S3 of
//! `.agents/04-agent-runtime/subagents.md`).
//!
//! One module per surface of a child's life: the ceilings it is charged
//! against ([`budget`]), the stop bit that ends it ([`cancel`]), the registry
//! its record lives in ([`registry`]), the handles it runs with ([`host`]) and
//! the loop that spends the budget ([`run`]). [`events`] turns a registry
//! transition into the S4 lifecycle events a turn merges into its own stream.
//!
//! A spawned sub-agent is a real child run: it has its own `chats` row (keyed
//! by its `SubAgentId`, carrying `parent_chat_id`), persists its user,
//! assistant and tool rows under its own id — never into the parent's
//! conversation — and works in its own cwd. It calls the same `LlmProvider`
//! the parent's turn uses (threaded through, never a second client) and
//! dispatches each child tool call through the harness's own
//! `execute_tool_call`; at the depth limit `spawn_subagent` is simply not in
//! the child's tool set.
//!
//! Every child runs on its own `tokio` task and is registered in the
//! [`SubAgentRegistry`] before it starts, so the spawn can either await the
//! child (foreground, up to a deadline after which it moves to the background
//! and keeps running) or return its id at once. The registered record is the
//! one source the renderer (S5–S7) and the wire events (S4) read: completion,
//! failure, budget exhaustion and cancellation all move it to a terminal
//! status, and a child whose loop unwinds is marked `Failed` by its task
//! rather than left live. A finished background child does not wake the
//! parent: no auto-wake policy is built here, the record is simply readable.

mod budget;
mod cancel;
mod events;
mod host;
mod registry;
mod run;

#[cfg(test)]
mod tests;

pub(crate) use budget::{default_child_budget, DEFAULT_FOREGROUND_CHILD_WAIT};
pub(crate) use cancel::PARENT_CANCEL_REASON;
pub(crate) use events::with_subagent_lifecycle;
pub(crate) use host::SubAgentHost;
pub(crate) use registry::SubAgentRegistry;
