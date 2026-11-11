//! The hub's automation plane: workflows, trigger verification, dispatch and
//! the audit trail. Execution stays on workers; the hub decides what runs
//! where and records what happened.

pub mod dispatch;
pub mod store;
pub mod trigger;
pub mod workflow;
