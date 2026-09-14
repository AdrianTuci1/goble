use std::time::Duration;

/// Where a live sub-agent child is, as the parent's transcript row reads it: the
/// four lines the row draws its own shape for. The record's own `initializing`
/// is a child that is live and has reported no activity yet, so it folds into
/// [`Self::Running`] rather than drawing a fifth line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubAgentRowStatus {
    #[default]
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SubAgentRowStatus {
    /// The status kind S4 carries on the wire (`goble_core::subagent_run`'s
    /// `status_kind`). An unrecognised kind leaves the child live: a row must
    /// not claim an outcome the record never reported.
    pub fn from_wire(kind: &str) -> Self {
        match kind {
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Running,
        }
    }

    /// The status bullet and its colour: S5's row affordance, owned by the
    /// status so the transcript row, the child view's title and the work
    /// overlay (S7) all draw the same mark for the same status rather than
    /// each inventing one.
    pub fn affordance(self) -> (&'static str, crate::theme::ColorToken) {
        match self {
            Self::Running => ("◐", crate::theme::ColorToken::Accent),
            Self::Completed => ("●", crate::theme::ColorToken::Success),
            Self::Failed => ("◆", crate::theme::ColorToken::Error),
            Self::Cancelled => ("◇", crate::theme::ColorToken::Muted),
        }
    }
}

/// A live sub-agent record, as the parent's transcript row draws it. The row's
/// data is this record and never the tool call's arguments: the app builds one
/// from S4's `chat:subagent_*` events (which mirror the record's own fields) and
/// keys it by the id of the parent tool call that spawned it.
#[derive(Clone, Debug, PartialEq)]
pub struct SubAgentRow {
    /// The child's own conversation id, which [`ChatAction::OpenSubAgent`](super::message::ChatAction::OpenSubAgent)
    /// carries so the app can open the child view (S6) for it.
    pub child_id: String,
    pub subagent_type: String,
    /// The one line the spawn recorded; the row's subject.
    pub description: String,
    pub status: SubAgentRowStatus,
    /// The live activity label. Empty unless the child is running.
    pub activity: String,
    /// The child's own elapsed time: the record's clock while it runs, its
    /// duration once it has ended.
    pub elapsed: Duration,
    pub turns: u32,
    pub tool_calls: u32,
    pub tokens: u64,
    /// What the child ended with: the output when it completed, the error or the
    /// cancellation reason otherwise. `None` while it runs.
    pub outcome: Option<String>,
    /// Whether the spawn asked for the result (`false`) or returned at once
    /// with the child's id (`true`).
    pub background: bool,
}

impl SubAgentRow {
    /// Whether the child is live, which is what puts the activity label and the
    /// ticking elapsed time on the row.
    pub fn is_running(&self) -> bool {
        self.status == SubAgentRowStatus::Running
    }

    /// The status bullet and its colour — S5's row affordance, exposed on the
    /// record so the child view's title and the work overlay carry the same
    /// mark for the same status rather than inventing a second one.
    pub fn status_affordance(&self) -> (&'static str, crate::theme::ColorToken) {
        self.status.affordance()
    }

    /// The row's status phrase, one per status, in grok-build's subagent shape:
    /// `running`, `completed in 43s`, `failed in 12s`, `cancelled in 5s`. A live
    /// child's activity and ticking elapsed time are the row's own further
    /// segments ([`Self::activity_segment`], [`Self::elapsed_segment`]).
    pub fn status_phrase(&self) -> String {
        match self.status {
            SubAgentRowStatus::Running => "running".to_string(),
            SubAgentRowStatus::Completed => {
                format!("completed in {}", format_subagent_elapsed(self.elapsed))
            }
            SubAgentRowStatus::Failed => {
                format!("failed in {}", format_subagent_elapsed(self.elapsed))
            }
            SubAgentRowStatus::Cancelled => {
                format!("cancelled in {}", format_subagent_elapsed(self.elapsed))
            }
        }
    }

    /// The activity segment of a running row: the child's own label, or
    /// `initializing` before it has reported one. `None` once the child ended.
    pub fn activity_segment(&self) -> Option<&str> {
        if !self.is_running() {
            return None;
        }
        if self.activity.is_empty() {
            return Some("initializing");
        }
        Some(self.activity.as_str())
    }

    /// The ticking elapsed time a running row carries. `None` when the child has
    /// ended — a terminal row's time is already inside [`Self::status_phrase`].
    pub fn elapsed_segment(&self) -> Option<String> {
        self.is_running()
            .then(|| format_subagent_elapsed(self.elapsed))
    }

    /// The folded row's whole status line: the status phrase, and while the
    /// child runs its activity label and its elapsed time, each `·`-joined. One
    /// line per status, and one row on the transcript.
    pub fn status_line(&self) -> String {
        let mut segments = vec![self.status_phrase()];
        segments.extend(self.activity_segment().map(str::to_string));
        segments.extend(self.elapsed_segment());
        if self.status == SubAgentRowStatus::Failed {
            if let Some(error) = self.outcome.as_deref().filter(|text| !text.is_empty()) {
                segments.push(error.to_string());
            }
        }
        segments.join(" · ")
    }

    /// The detail row the expanded body carries: what the record has counted,
    /// and how the child was spawned.
    pub fn counters_line(&self) -> String {
        let spawn = if self.background {
            "background"
        } else {
            "foreground"
        };
        format!(
            "{} turns · {} tool calls · {} tokens · {spawn}",
            self.turns, self.tool_calls, self.tokens
        )
    }
}

/// A sub-agent's elapsed time, in grok-build's `format_duration` shape: one
/// decimal under ten seconds, whole seconds under a minute, then minutes/hours.
pub(crate) fn format_subagent_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 10 {
        return format!("{:.1}s", elapsed.as_secs_f64());
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    let (minutes, seconds) = (secs / 60, secs % 60);
    if minutes < 60 {
        return format!("{minutes}m{seconds:02}s");
    }
    format!("{}h{:02}m", minutes / 60, minutes % 60)
}
