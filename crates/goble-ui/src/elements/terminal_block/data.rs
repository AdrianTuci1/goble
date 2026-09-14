use crate::color::ColorU;
use crate::elements::AppContext;
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;
use goble_terminal::{Palette, ScreenColor, ScreenLine, Underline};
use super::block::TerminalBlock;
use super::line::{TerminalLine, TerminalLineKind, TerminalRun};

/// Serializable data for embedding a terminal block inside a chat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalData {
    pub title: String,
    pub lines: Vec<TerminalLine>,
    pub status: Option<TerminalStatus>,
    /// The block's own context, drawn at the top of the section beside the
    /// title. `None` for a block whose header carries only its title.
    pub meta: Option<TerminalMeta>,
}

/// What a block's header says about where it ran and how long it took
/// (warp-new's block label): the working directory, the git branch and the
/// command's duration, all preformatted by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalMeta {
    pub path: String,
    pub branch: Option<String>,
    pub duration: Option<String>,
}

impl TerminalMeta {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            branch: None,
            duration: None,
        }
    }

    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    pub fn with_duration(mut self, duration: impl Into<String>) -> Self {
        self.duration = Some(duration.into());
        self
    }

    /// The labels the header draws, in order. An empty path is left out, so a
    /// block whose shell never reported a working directory draws no empty
    /// label for it.
    pub(crate) fn labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        if !self.path.is_empty() {
            labels.push(self.path.clone());
        }
        if let Some(branch) = &self.branch {
            labels.push(format!("git:({branch})"));
        }
        if let Some(duration) = &self.duration {
            labels.push(duration.clone());
        }
        labels
    }
}

impl TerminalData {
    pub fn new(title: impl Into<String>, lines: Vec<TerminalLine>) -> Self {
        Self {
            title: title.into(),
            lines,
            status: None,
            meta: None,
        }
    }

    pub fn with_status(mut self, status: TerminalStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_meta(mut self, meta: TerminalMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// The section a command of the pane's own history is drawn as: a header
    /// carrying the block's context (where it ran, how long it took) over the
    /// command line and its output.
    ///
    /// Unlike [`TerminalData::for_command`] the command is not repeated in the
    /// header: the section's top line is the context, and the command follows it
    /// the way a shell prompt line follows warp's block label.
    pub fn for_section(
        command: impl Into<String>,
        output: &str,
        meta: TerminalMeta,
        status: TerminalStatus,
    ) -> Self {
        let mut section = Self::for_command(command, output, status).with_meta(meta);
        // The command is drawn once, on the section's own `❯` line. A header
        // title repeating it would say the same thing twice, so the header is
        // only the block's context.
        section.title = String::new();
        section
    }

    /// The block a command produces: the command line, then its output, with the
    /// command's state carried as the block's status.
    ///
    /// A command the agent ran and one that ran in the pane are the same block
    /// shape, so both are drawn by [`terminal_block`](super::block::terminal_block) — one block, one renderer.
    pub fn for_command(command: impl Into<String>, output: &str, status: TerminalStatus) -> Self {
        let command = command.into();
        let mut lines = vec![TerminalLine::styled(
            command.clone(),
            TerminalLineKind::Command,
            command_runs(&command),
        )];
        for line in goble_terminal::ansi::render_lines(output.as_bytes()) {
            let text = line.text();
            if text.is_empty() {
                lines.push(TerminalLine::info(" "));
            } else {
                lines.push(TerminalLine::styled(
                    text,
                    TerminalLineKind::Output,
                    output_runs(&line),
                ));
            }
        }
        Self::new(command, lines).with_status(status)
    }

    /// A stable per-block key (title + line text) used to look up the block's
    /// app-owned filter state in [`crate::elements::ChatView`]'s filter map.
    /// Two terminal blocks with identical content share a filter state, which
    /// is acceptable — content-derived keys reset as streaming output changes.
    pub fn filter_key(&self) -> String {
        let mut key = self.title.clone();
        for line in &self.lines {
            key.push('\n');
            key.push_str(&line.text);
        }
        key
    }

    /// The whole block as display text (the ❯ prompt is included for commands),
    /// used by the copy button.
    pub fn full_text(&self) -> String {
        self.lines
            .iter()
            .map(|line| TerminalBlock::line_text(line))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Optional status shown in the top-right corner of the terminal header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TerminalStatus {
    #[default]
    Idle,
    Running,
    Success,
    Error,
}

impl TerminalStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "",
            Self::Running => "running",
            Self::Success => "done",
            Self::Error => "error",
        }
    }

    pub fn color(self, app: &AppContext) -> ColorU {
        match self {
            Self::Idle => app.theme.color(ColorToken::Muted),
            Self::Running => app.theme.color(ColorToken::Accent),
            Self::Success => app.theme.color(ColorToken::Success),
            Self::Error => app.theme.color(ColorToken::Error),
        }
    }
}

/// The command line highlighted as shell, through H1's highlighter. `bash` is
/// the token a command carries. An unresolved language, or a command the
/// highlighter splits across lines, leaves the line plain rather than drawing a
/// fragment of it.
fn command_runs(command: &str) -> Vec<TerminalRun> {
    let Some(lines) = crate::syntax::highlight(command, "bash") else {
        return Vec::new();
    };
    if lines.len() != 1 {
        return Vec::new();
    }
    let mut runs = Vec::new();
    for span in &lines[0] {
        push_run(
            &mut runs,
            span.text.clone(),
            Some(span.color),
            span.weight == FontWeight::Bold,
            span.italic,
            false,
        );
    }
    runs
}

/// A parsed output line's styled runs. A line that is entirely the terminal's
/// default yields no runs, so the ordinary case is still drawn as one plain
/// `Text` run and only coloured output changes.
fn output_runs(line: &ScreenLine) -> Vec<TerminalRun> {
    let palette = Palette::default();
    let mut runs = Vec::new();
    for cell in &line.cells {
        if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
            continue;
        }
        let mut text = String::new();
        text.push(cell.ch);
        for mark in &cell.zerowidth {
            text.push(*mark);
        }
        push_run(
            &mut runs,
            text,
            run_color(cell.fg, &palette),
            cell.attrs.bold,
            cell.attrs.italic,
            cell.attrs.underline != Underline::None,
        );
    }
    if runs.len() == 1 {
        let only = &runs[0];
        if only.color.is_none() && !only.bold && !only.italic && !only.underline {
            return Vec::new();
        }
    }
    runs
}

/// A cell's foreground as a painted colour. The terminal's default colours have
/// no fixed value, so they are left to the renderer's theme.
fn run_color(color: ScreenColor, palette: &Palette) -> Option<ColorU> {
    if color.is_default() {
        return None;
    }
    let (r, g, b) = palette.rgb(color);
    Some(ColorU::new(r, g, b, 255))
}

/// Append a run, merging it with the previous one when they share a style so a
/// plain stretch of output stays one run.
fn push_run(
    runs: &mut Vec<TerminalRun>,
    text: String,
    color: Option<ColorU>,
    bold: bool,
    italic: bool,
    underline: bool,
) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut() {
        if last.color == color
            && last.bold == bold
            && last.italic == italic
            && last.underline == underline
        {
            last.text.push_str(&text);
            return;
        }
    }
    runs.push(TerminalRun {
        text,
        color,
        bold,
        italic,
        underline,
    });
}
