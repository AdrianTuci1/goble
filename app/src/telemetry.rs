//! The app's outcome sites, expressed in the telemetry catalog.
//!
//! Every product event the app reports goes through this module, so the
//! vocabulary stays the closed enum in `goble_telemetry` and no call site can
//! invent a name, a free-text outcome, or user content. These functions are
//! no-ops when telemetry did not start.

use std::time::Duration;

use goble_core::harness::{CommandDecision, ToolCallStatus};
use goble_telemetry::{Feature, Outcome};

use crate::state::TurnOutcome;

/// A turn settled. The first turn on an installation is reported once.
pub fn turn_finished(outcome: &TurnOutcome, duration: Option<Duration>) {
    let outcome = match outcome {
        TurnOutcome::Success => Outcome::Success,
        TurnOutcome::Failed(_) => Outcome::Error,
        // The harness said the turn was over without saying how. Calling that
        // a success would invent a rate nobody measured.
        TurnOutcome::Unknown => Outcome::Error,
    };
    goble_telemetry::feature(Feature::TurnCompleted, outcome, duration);
    goble_telemetry::feature_once(Feature::FirstTurnCompleted, outcome, duration);
}

/// A live tool-call transition, reported only when it settled.
///
/// The live event carries no start time, so the duration stays unknown rather
/// than being measured from when the app happened to see it.
pub fn tool_call_finished(status: ToolCallStatus) {
    let outcome = match status {
        ToolCallStatus::Finished => Outcome::Success,
        ToolCallStatus::Error => Outcome::Error,
        ToolCallStatus::Pending | ToolCallStatus::Running => return,
    };
    goble_telemetry::feature(Feature::ToolCallFinished, outcome, None);
}

/// The user answered a command proposal: approved, edited or refused.
pub fn approval_decision(decision: &CommandDecision) {
    let outcome = match decision {
        CommandDecision::Approve(_) | CommandDecision::Edit(_) => Outcome::Success,
        CommandDecision::Reject(_) => Outcome::Denied,
    };
    goble_telemetry::feature(Feature::ApprovalDecision, outcome, None);
}

/// A proposal that was dropped without the user answering it: a new message
/// superseded it. Reporting that as a denial would read as the user said no.
pub fn approval_superseded() {
    goble_telemetry::feature(Feature::ApprovalDecision, Outcome::Cancelled, None);
}

/// A terminal command block finished.
///
/// `by_user` separates a command a person typed from one the agent ran: only
/// the first one is the installation's "first terminal command".
pub fn terminal_command_finished(outcome: Outcome, duration: Option<Duration>, by_user: bool) {
    goble_telemetry::feature(Feature::TerminalCommandRun, outcome, duration);
    if by_user {
        goble_telemetry::feature_once(Feature::FirstTerminalCommand, outcome, duration);
    }
}

/// First-run onboarding was finished or deliberately skipped.
pub fn onboarding_completed() {
    goble_telemetry::feature_once(Feature::OnboardingCompleted, Outcome::Success, None);
}

/// One update check ran, whether or not an update was found.
pub fn update_check(outcome: Outcome, duration: Option<Duration>) {
    goble_telemetry::feature(Feature::UpdateCheck, outcome, duration);
}
