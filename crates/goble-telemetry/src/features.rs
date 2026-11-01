//! The closed vocabulary of product signal.
//!
//! A feature name is an enum, not a string, so a typo at a call site is a
//! compile error instead of a new series that quietly splits the data. The
//! wire names are the snake_case forms below and they are fixed: renaming a
//! variant changes what the collector and the Athena views have already
//! recorded under the old name.
//!
//! Every feature carries an [`Outcome`] and, when it was measured, a duration
//! in milliseconds. Nothing here carries user content: a feature name says that
//! a turn finished, never what was asked.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Product events worth counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// The user finished (or deliberately skipped) first-run onboarding.
    OnboardingCompleted,
    /// The first agent turn on this installation settled.
    FirstTurnCompleted,
    /// The first shell command on this installation finished.
    FirstTerminalCommand,
    /// A turn settled. One per user prompt, not per model step.
    TurnCompleted,
    /// One tool call settled, with how long it ran.
    ToolCallFinished,
    /// The user allowed or denied a command the harness proposed.
    ApprovalDecision,
    /// A command block finished in a terminal pane.
    TerminalCommandRun,
    /// A session ended; the duration is its length.
    SessionSummary,
    /// An update check ran, whether or not an update was found.
    UpdateCheck,
    /// A crash was captured locally.
    Crash,
}

impl Feature {
    /// Every feature, so a test can prove the names are unique and a config
    /// can iterate the catalog.
    pub const ALL: [Feature; 10] = [
        Feature::OnboardingCompleted,
        Feature::FirstTurnCompleted,
        Feature::FirstTerminalCommand,
        Feature::TurnCompleted,
        Feature::ToolCallFinished,
        Feature::ApprovalDecision,
        Feature::TerminalCommandRun,
        Feature::SessionSummary,
        Feature::UpdateCheck,
        Feature::Crash,
    ];

    /// The wire name. Stable: changing one breaks the series it names.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnboardingCompleted => "onboarding_completed",
            Self::FirstTurnCompleted => "first_turn_completed",
            Self::FirstTerminalCommand => "first_terminal_command",
            Self::TurnCompleted => "turn_completed",
            Self::ToolCallFinished => "tool_call_finished",
            Self::ApprovalDecision => "approval_decision",
            Self::TerminalCommandRun => "terminal_command_run",
            Self::SessionSummary => "session_summary",
            Self::UpdateCheck => "update_check",
            Self::Crash => "crash",
        }
    }

    /// The feature with this wire name, if the catalog has it.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|feature| feature.as_str() == name)
    }

    /// Whether this event may happen at most once per installation. The flag
    /// lives in [`crate::TelemetryState`], so a reinstall is a new "first".
    pub fn once_per_installation(self) -> bool {
        matches!(
            self,
            Self::OnboardingCompleted | Self::FirstTurnCompleted | Self::FirstTerminalCommand
        )
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a feature ended. Free text never belongs here: an outcome is one of
/// four states, which is what makes a success rate computable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Error,
    Cancelled,
    /// Refused by the user or by policy, before or instead of running.
    Denied,
}

impl Outcome {
    pub const ALL: [Outcome; 4] =
        [Outcome::Success, Outcome::Error, Outcome::Cancelled, Outcome::Denied];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Denied => "denied",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|outcome| outcome.as_str() == name)
    }

    /// Whether the feature did what the user asked for.
    pub fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_catalog_covers_every_feature_exactly_once() {
        let mut seen = BTreeSet::new();
        for feature in Feature::ALL {
            assert!(seen.insert(feature), "{feature} is listed twice");
        }
        assert_eq!(seen.len(), Feature::ALL.len());

        // Adding a variant without adding it to `ALL` would silently drop it
        // from config iteration and from `from_name`.
        for name in [
            "onboarding_completed",
            "first_turn_completed",
            "first_terminal_command",
            "turn_completed",
            "tool_call_finished",
            "approval_decision",
            "terminal_command_run",
            "session_summary",
            "update_check",
            "crash",
        ] {
            assert!(Feature::from_name(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn feature_names_are_unique_and_round_trip() {
        let mut names = BTreeSet::new();
        for feature in Feature::ALL {
            assert!(names.insert(feature.as_str()), "{} is not unique", feature.as_str());
            assert_eq!(Feature::from_name(feature.as_str()), Some(feature));
            assert!(feature.as_str().chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }

    #[test]
    fn outcome_names_are_unique_and_round_trip() {
        let mut names = BTreeSet::new();
        for outcome in Outcome::ALL {
            assert!(names.insert(outcome.as_str()), "{} is not unique", outcome.as_str());
            assert_eq!(Outcome::from_name(outcome.as_str()), Some(outcome));
        }
    }

    #[test]
    fn the_wire_name_of_a_feature_is_its_enum_name() {
        // Pinned on purpose: these strings are a data contract with Athena.
        assert_eq!(Feature::TurnCompleted.as_str(), "turn_completed");
        assert_eq!(Feature::TerminalCommandRun.as_str(), "terminal_command_run");
        assert_eq!(Feature::from_name("turn_completed"), Some(Feature::TurnCompleted));
        assert_eq!(Feature::from_name("TurnCompleted"), None);
    }

    #[test]
    fn only_the_three_firsts_are_once_per_installation() {
        for feature in Feature::ALL {
            assert_eq!(
                feature.once_per_installation(),
                matches!(
                    feature,
                    Feature::OnboardingCompleted
                        | Feature::FirstTurnCompleted
                        | Feature::FirstTerminalCommand
                ),
                "{feature}"
            );
        }
    }

    #[test]
    fn a_feature_and_outcome_serialise_as_their_wire_names() {
        assert_eq!(serde_json::to_string(&Feature::Crash).unwrap(), "\"crash\"");
        assert_eq!(serde_json::to_string(&Outcome::Denied).unwrap(), "\"denied\"");
        assert_eq!(serde_json::from_str::<Outcome>("\"cancelled\"").unwrap(), Outcome::Cancelled);
    }
}
