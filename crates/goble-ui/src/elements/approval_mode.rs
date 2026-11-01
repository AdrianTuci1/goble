//! The approval mode the rich input toggles.
//!
//! Warp-new's autonomy control: how a command the agent proposes reaches the
//! shell. The app keeps one mode for every pane — the composer draws it beside
//! the model select, the settings page steps it, and the choice is persisted.

use crate::theme::ColorToken;

/// How the pane treats a command the harness suspends on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Run every proposed command without asking. The turn never suspends on
    /// the user; the approval card is not raised.
    AlwaysApprove,
    /// Ask before every command: the approval card waits at the end of the
    /// agent's output until the user runs or rejects it.
    #[default]
    Manual,
    /// Plan only: commands are refused, so the turn continues without touching
    /// the workspace.
    Plan,
    /// Review every command: the card waits for the user's decision. The
    /// separate reviewer pass is the harness's to add; until it is wired the
    /// mode behaves as [`Self::Manual`].
    AutoReview,
}

impl ApprovalMode {
    pub const ALL: [Self; 4] = [
        Self::AlwaysApprove,
        Self::Manual,
        Self::Plan,
        Self::AutoReview,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::AlwaysApprove => "always-approve",
            Self::Manual => "manual",
            Self::Plan => "plan",
            Self::AutoReview => "auto-review",
        }
    }

    /// The colour of the mode's name in the rich input. The control draws no
    /// glyph; the hue is what tells the modes apart at a glance — green for the
    /// policy that runs commands without asking, amber for the one that only
    /// plans, accent for the reviewer, and the ordinary text colour for manual.
    pub fn color(self) -> ColorToken {
        match self {
            Self::AlwaysApprove => ColorToken::Success,
            Self::Manual => ColorToken::Text,
            Self::Plan => ColorToken::Warning,
            Self::AutoReview => ColorToken::Accent,
        }
    }

    /// What the mode means, said on the control's tooltip.
    pub fn description(self) -> &'static str {
        match self {
            Self::AlwaysApprove => "Run proposed commands without asking",
            Self::Manual => "Ask before every proposed command",
            Self::Plan => "Plan only: refuse proposed commands",
            Self::AutoReview => "Review every proposed command",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(1)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Manual)
    }

    /// The mode a stored setting names. An unknown or missing value is
    /// [`Self::Manual`], the policy that asks before running anything.
    pub fn from_label(value: &str) -> Self {
        let value = value.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|mode| mode.label() == value)
            .unwrap_or(Self::Manual)
    }

    pub fn next(self) -> Self {
        Self::from_index((self.index() + 1) % Self::ALL.len())
    }

    pub fn previous(self) -> Self {
        Self::from_index((self.index() + Self::ALL.len() - 1) % Self::ALL.len())
    }

    /// Whether a proposal waits for the user in this mode.
    pub fn waits_for_user(self) -> bool {
        matches!(self, Self::Manual | Self::AutoReview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_round_trips_through_the_setting() {
        for mode in ApprovalMode::ALL {
            assert_eq!(ApprovalMode::from_label(mode.label()), mode, "{mode:?}");
        }
    }

    #[test]
    fn an_unknown_setting_falls_back_to_manual() {
        assert_eq!(ApprovalMode::from_label(""), ApprovalMode::Manual);
        assert_eq!(ApprovalMode::from_label("yolo"), ApprovalMode::Manual);
    }

    #[test]
    fn stepping_walks_the_modes_both_ways_and_wraps() {
        let mut mode = ApprovalMode::AlwaysApprove;
        for _ in 0..ApprovalMode::ALL.len() {
            mode = mode.next();
        }
        assert_eq!(mode, ApprovalMode::AlwaysApprove);
        assert_eq!(
            ApprovalMode::AlwaysApprove.previous(),
            ApprovalMode::AutoReview
        );
    }
}
