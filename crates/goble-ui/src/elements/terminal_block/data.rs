use crate::color::ColorU;
use crate::elements::AppContext;
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;
use goble_terminal::{Palette, ScreenCell, ScreenLine};
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
    /// The prompt the command was typed at, drawn at the head of the header.
    /// When set it replaces `path`, because the prompt already names the
    /// directory the command ran in — the shell prompt's own shape.
    pub prompt: Option<String>,
}

impl TerminalMeta {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            branch: None,
            duration: None,
            prompt: None,
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

    /// Attach the shell prompt the command was typed at. The block's header
    /// draws it where its directory label would go; the body draws no prompt
    /// row of its own.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// The labels the header draws, in order. An empty path is left out, so a
    /// block whose shell never reported a working directory draws no empty
    /// label for it. A prompt replaces the path label, not adds to it.
    pub(crate) fn labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        match &self.prompt {
            Some(prompt) if !prompt.is_empty() => labels.push(prompt.clone()),
            _ if !self.path.is_empty() => labels.push(self.path.clone()),
            _ => {}
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
        Self::section_of(Self::for_command(command, output, status), meta)
    }

    /// The section a block's own screen draws: the same shape as
    /// [`TerminalData::for_section`], but read from the cells the command
    /// painted into rather than from its text, so the colours, backgrounds and
    /// attributes survive into the drawn runs.
    pub fn for_section_lines(
        command: impl Into<String>,
        output: &[ScreenLine],
        palette: Palette,
        meta: TerminalMeta,
        status: TerminalStatus,
    ) -> Self {
        Self::section_of(Self::for_command_lines(command, output, palette, status), meta)
    }

    /// The block a command produces: the command line, then its output, with the
    /// command's state carried as the block's status.
    ///
    /// A command the agent ran and one that ran in the pane are the same block
    /// shape, so both are drawn by [`terminal_block`](super::block::terminal_block) — one block, one renderer.
    pub fn for_command(command: impl Into<String>, output: &str, status: TerminalStatus) -> Self {
        Self::for_command_lines(
            command,
            &goble_terminal::ansi::render_lines(output.as_bytes()),
            Palette::default(),
            status,
        )
    }

    /// The same block, from output the screen already parsed. The palette is
    /// what the block was painted with, so the three default colours resolve the
    /// way the pane's grid resolves them.
    pub fn for_command_lines(
        command: impl Into<String>,
        output: &[ScreenLine],
        palette: Palette,
        status: TerminalStatus,
    ) -> Self {
        let command = command.into();
        let mut lines = vec![TerminalLine::styled(
            command.clone(),
            TerminalLineKind::Command,
            command_runs(&command),
        )];
        lines.extend(output_lines(output, &palette));
        Self::new(command, lines).with_status(status)
    }

    /// A block of output with no command line of its own, read from cells: the
    /// live screen a shell that has not delimited a block yet leaves behind.
    pub fn from_output_lines(
        title: impl Into<String>,
        output: &[ScreenLine],
        palette: Palette,
        status: TerminalStatus,
    ) -> Self {
        Self::new(title, output_lines(output, &palette)).with_status(status)
    }

    fn section_of(mut section: Self, meta: TerminalMeta) -> Self {
        section.meta = Some(meta);
        // The command is drawn once, on the section's own `❯` line. A header
        // title repeating it would say the same thing twice, so the header is
        // only the block's context.
        section.title = String::new();
        section
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
    let mut runs: Vec<TerminalRun> = Vec::new();
    for span in &lines[0] {
        let run = TerminalRun {
            text: span.text.clone(),
            color: Some(span.color),
            background: None,
            bold: span.weight == FontWeight::Bold,
            dim: false,
            italic: span.italic,
            underline: false,
            strikeout: false,
        };
        push_run(&mut runs, run);
    }
    runs
}

/// A screen's output rows as the block draws them: each row's text and the runs
/// its cells produced. A row that draws nothing is the single space the block
/// keeps for a blank line, so the rows stay aligned with the block's text.
fn output_lines(output: &[ScreenLine], palette: &Palette) -> Vec<TerminalLine> {
    output
        .iter()
        .map(|line| {
            let text = line.text();
            let runs = output_runs(line, palette);
            if text.is_empty() && runs.is_empty() {
                TerminalLine::info(" ")
            } else {
                TerminalLine::styled(text, TerminalLineKind::Output, runs)
            }
        })
        .collect()
}

/// A parsed output line's styled runs. A line that is entirely the terminal's
/// default yields no runs, so the ordinary case is still drawn as one plain
/// `Text` run and only styled output changes.
///
/// Adjacent cells of one style become one run, so a word, a coloured stretch or
/// a background band is one draw; cells whose style differs split it. Wide
/// characters keep their one glyph (the spacer cell that follows them carries
/// no glyph of its own) and their advance comes from the font, so a TUI's box
/// drawing and emoji keep the grid's shape.
fn output_runs(line: &ScreenLine, palette: &Palette) -> Vec<TerminalRun> {
    let mut runs: Vec<TerminalRun> = Vec::new();
    let mut pending: Option<TerminalRun> = None;

    let flush = |run: &mut Option<TerminalRun>, runs: &mut Vec<TerminalRun>| {
        if let Some(run) = run.take() {
            if !run.text.is_empty() {
                push_run(runs, run);
            }
        }
    };

    // A row's trailing blanks paint nothing: the block's own background shows
    // through them, and keeping them would draw a run of spaces on every line.
    // A blank cell that carries a background or a rule is not trailing filler.
    let end = line
        .cells
        .iter()
        .rposition(|cell| paints(cell))
        .map_or(0, |index| index + 1);

    for cell in &line.cells[..end] {
        if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
            continue;
        }
        let style = cell_run(cell, palette);
        let mut text = String::new();
        text.push(cell.ch);
        text.extend(cell.zerowidth.iter());
        match &mut pending {
            Some(run) if run.same_style(&style) => run.text.push_str(&text),
            _ => {
                flush(&mut pending, &mut runs);
                pending = Some(TerminalRun { text, ..style });
            }
        }
    }
    flush(&mut pending, &mut runs);

    if runs.len() == 1 {
        let only = &runs[0];
        if only.color.is_none()
            && only.background.is_none()
            && !only.bold
            && !only.dim
            && !only.italic
            && !only.underline
            && !only.strikeout
        {
            return Vec::new();
        }
    }
    runs
}

/// Whether a cell draws anything at all: a glyph, a mark, a background or a
/// rule. A blank cell of the terminal's own colours is not part of the row.
fn paints(cell: &ScreenCell) -> bool {
    if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
        return false;
    }
    cell.ch != ' '
        || !cell.zerowidth.is_empty()
        || !cell.bg.is_default()
        || cell.attrs.inverse
        || cell.attrs.hidden
        || cell.attrs.underline != goble_terminal::Underline::None
        || cell.attrs.strikeout
}

/// A cell's drawn style. Inverse swaps the two colours; hidden paints the text
/// in the background colour; dim darkens it. The three default colours have no
/// fixed value, so they are left to the renderer's theme — except under inverse,
/// where the theme's own foreground becomes the cell's background and has to be
/// painted.
fn cell_run(cell: &ScreenCell, palette: &Palette) -> TerminalRun {
    let (foreground, background) = palette.cell_colors(cell);
    let default_text = cell.fg.is_default()
        && !cell.attrs.inverse
        && !cell.attrs.dim
        && !cell.attrs.hidden;
    let default_background = cell.bg.is_default() && !cell.attrs.inverse;
    TerminalRun {
        text: String::new(),
        color: (!default_text).then(|| rgb(foreground)),
        background: (!default_background).then(|| rgb(background)),
        bold: cell.attrs.bold,
        dim: cell.attrs.dim,
        italic: cell.attrs.italic,
        underline: cell.attrs.underline != goble_terminal::Underline::None,
        strikeout: cell.attrs.strikeout,
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> ColorU {
    ColorU::new(r, g, b, 255)
}

/// Append a run, merging it with the previous one when they share a style so a
/// plain stretch of output stays one run.
fn push_run(runs: &mut Vec<TerminalRun>, mut run: TerminalRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut() {
        if last.same_style(&run) {
            last.text.push_str(&run.text);
            return;
        }
    }
    run.text.shrink_to_fit();
    runs.push(run);
}
