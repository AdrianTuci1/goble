//! The approval mode the rich input toggles.
//!
//! Warp-new's autonomy control: how a command the agent proposes reaches the
//! shell. The mode belongs to the pane, so two conversations can run under
//! different policies, and the composer draws it beside the model select.

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
        Self::ALL
            .iter()
            .position(|mode| *mode == self)
            .unwrap_or(1)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Manual)
    }

    pub fn next(self) -> Self {
        Self::from_index((self.index() + 1) % Self::ALL.len())
    }

    /// Whether a proposal waits for the user in this mode.
    pub fn waits_for_user(self) -> bool {
        matches!(self, Self::Manual | Self::AutoReview)
    }
}
