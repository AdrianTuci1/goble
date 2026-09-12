//! The reasoning loop: one mission planned in explicit thinking steps, then
//! executed with the harness's own tools.
//!
//! One module per surface of a mission: its types ([`types`]), the extra tools
//! the loop offers the model ([`tools`]), the prompts it builds ([`prompts`])
//! and the rows it persists ([`persist`]); the turn itself ([`mission`]), the
//! resumes that pick a suspended turn back up ([`resume`]), the transcript a
//! resumed turn replays ([`history`]) and the questions asked before a step
//! ([`classify`]).
//!
//! A mission lives in the store: its goal, status and plan, its reasoning
//! steps (each one a mode, a decision and the tool calls it produced) and any
//! `ask_user` or proposed command it is waiting on. The turn advances the
//! mission one decision at a time; a suspended turn is resumed from the
//! persisted state rather than from memory.

mod classify;
mod history;
mod mission;
mod persist;
mod prompts;
mod resume;
mod tools;
mod types;

#[cfg(test)]
mod tests;

pub use tools::build_reasoning_tools;
pub use types::{MissionState, PendingAsk, ReasoningDecision, ReasoningStep};

pub use resume::resume_command_turn;

pub(crate) use mission::run_mission_turn;
pub(crate) use resume::resume_mission_turn;
