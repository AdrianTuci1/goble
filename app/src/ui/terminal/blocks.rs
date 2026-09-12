//! The block an executed command is drawn as, shared with the transcript.

use std::time::Duration;

use goble_terminal::blocks::BlockView;
use goble_ui::elements::{TerminalData, TerminalLine, TerminalMeta, TerminalStatus};

use crate::emulator::VisibleBlock;
use crate::terminal::TerminalSession;

/// The block a pane's executed command is drawn as: the live session's output,
/// titled for the pane.
///
/// This is the pane's own block path, and the transcript reuses it: a command
/// that ran in the pane is drawn with the same terminal block the transcript
/// draws a command the agent ran with, so one command is one block in both
/// places. The status is the newest command block's own lifecycle rather than
/// an assumed success, so a command still running is not drawn as finished.
/// `None` while the session has drawn nothing.
pub fn executed_command_block(session: &TerminalSession) -> Option<TerminalData> {
    let snapshot = session.snapshot(48);
    if snapshot.lines.is_empty() {
        return None;
    }
    let lines = snapshot
        .lines
        .iter()
        .map(|line| {
            let text = line.trim_end().to_string();
            if text.is_empty() {
                TerminalLine::info(" ")
            } else {
                TerminalLine::output(text)
            }
        })
        .collect();
    // The live screen belongs to the newest command, and a finished command
    // leaves an empty prompt block behind it, so the block to read is the
    // newest one that actually carries a command.
    let status = session
        .visible_blocks(&BlockView::Terminal)
        .into_iter()
        .rev()
        .find(|block| block.card.is_none() && !block.command.is_empty())
        .map_or(TerminalStatus::Success, |block| block_status(&block));
    Some(TerminalData::new("terminal", lines).with_status(status))
}

/// The status a block draws, read from its lifecycle and exit code rather than
/// from a boolean: a block that has not finished is still running, so it is
/// never painted as a success, and only a finished non-zero exit is an error.
pub(super) fn block_status(block: &VisibleBlock) -> TerminalStatus {
    if !block.state.is_done() {
        return TerminalStatus::Running;
    }
    match block.exit_code {
        Some(code) if code != 0 => TerminalStatus::Error,
        _ => TerminalStatus::Success,
    }
}

/// One section of the pane's own history: the command as a block, with the
/// context it ran in on the section's top line.
///
/// `None` for a block that is not a command section at all — a conversation
/// card (drawn as a card) or one of the shell's own prompt blocks, which the
/// block view hides rather than draws as an empty section.
pub fn section_data(block: &VisibleBlock) -> Option<TerminalData> {
    let command = block.command.trim_end();
    if block.card.is_some() || command.is_empty() {
        return None;
    }
    let mut meta = TerminalMeta::new(crate::state::display_path(
        block.pwd.as_deref().unwrap_or_default(),
    ));
    if let Some(branch) = block.git_branch.as_deref().filter(|b| !b.is_empty()) {
        meta = meta.with_branch(branch);
    }
    if let Some(duration) = block.duration {
        meta = meta.with_duration(duration_text(duration));
    }
    Some(TerminalData::for_section(
        command,
        &block.output,
        meta,
        block_status(block),
    ))
}

/// A command's duration the way warp's block label writes it: seconds under a
/// minute, minutes and seconds under an hour, and hours above.
pub fn duration_text(duration: Duration) -> String {
    let millis = duration.as_millis() as f64;
    let seconds = millis / 1000.0;
    let minutes = (seconds / 60.0) as u64;
    if duration.as_secs() >= 3600 {
        format!(
            "({}h {}m {:.0}s)",
            duration.as_secs() / 3600,
            minutes % 60,
            seconds % 60.0
        )
    } else if minutes > 0 {
        format!("({}m {:.2}s)", minutes, seconds % 60.0)
    } else {
        format!("({seconds:.3}s)")
    }
}
