//! The block an executed command is drawn as, shared with the transcript.

use std::time::Duration;

use goble_terminal::blocks::BlockView;
use goble_terminal::{Palette, ScreenLine};
use goble_ui::elements::{TerminalData, TerminalMeta, TerminalStatus};

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
    // An integrated shell reports its commands as blocks, so the live screen is
    // not the source: the screen carries the prompt row too, and a prompt
    // belongs in the block's header. The newest block with a command is the
    // block this view draws, streaming as its output grows.
    if session.is_bootstrapped() {
        let view = BlockView::Terminal;
        let block = session
            .visible_blocks(&view)
            .into_iter()
            .rev()
            .find(|block| block.card.is_none() && !block.command.trim().is_empty())?;
        let cells = session.block_output_lines_of(block.id);
        return section_data_with(&block, cells.as_deref(), session.palette());
    }

    // A shell that has not delimited a block yet: the live screen is the block,
    // and its rows keep the colours the programs on it painted.
    let mut rows = session.view().rows;
    while rows.last().is_some_and(ScreenLine::is_blank) {
        rows.pop();
    }
    if rows.len() > 48 {
        rows.drain(..rows.len() - 48);
    }
    if rows.is_empty() {
        return None;
    }
    // The live screen belongs to the newest command, and a finished command
    // leaves an empty prompt block behind it, so the block to read is the
    // newest one that actually carries a command.
    let status = session
        .visible_blocks(&BlockView::Terminal)
        .into_iter()
        .rev()
        .find(|block| block.card.is_none() && !block.command.is_empty())
        .map_or(TerminalStatus::Success, |block| block_status(&block));
    Some(TerminalData::from_output_lines(
        "terminal",
        &rows,
        session.palette(),
        status,
    ))
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
    section_data_with(block, None, Palette::xterm())
}

/// The same section, with the block's own output cells: the command's screen
/// keeps the colours, backgrounds and attributes the program painted, and the
/// plain [`VisibleBlock::output`] text has already lost them.
///
/// `palette` is the palette the block was painted with, so a cell that asked for
/// one of the three default colours resolves the way the pane's grid does.
pub fn styled_section_data(
    block: &VisibleBlock,
    output: &[ScreenLine],
    palette: Palette,
) -> Option<TerminalData> {
    section_data_with(block, Some(output), palette)
}

fn section_data_with(
    block: &VisibleBlock,
    output: Option<&[ScreenLine]>,
    palette: Palette,
) -> Option<TerminalData> {
    let command = block.command.trim_end();
    if block.card.is_some() || command.is_empty() {
        return None;
    }
    let mut meta = TerminalMeta::new(crate::state::display_path(
        block.pwd.as_deref().unwrap_or_default(),
    ));
    if let Some(prompt) = block.prompt.as_deref().filter(|p| !p.is_empty()) {
        meta = meta.with_prompt(prompt);
    }
    if let Some(branch) = block.git_branch.as_deref().filter(|b| !b.is_empty()) {
        meta = meta.with_branch(branch);
    }
    if let Some(duration) = block.duration {
        meta = meta.with_duration(duration_text(duration));
    }
    Some(match output {
        Some(lines) => TerminalData::for_section_lines(
            command,
            lines,
            palette,
            meta,
            block_status(block),
        ),
        None => TerminalData::for_section(command, &block.output, meta, block_status(block)),
    })
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
