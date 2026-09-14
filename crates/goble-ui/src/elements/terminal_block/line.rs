use crate::color::ColorU;

/// How a terminal line should be styled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalLineKind {
    Command,
    Output,
    Info,
    Success,
    Error,
}

/// A styled run of a terminal line: the text a command emitted and the
/// foreground, weight and emphasis in force when it did.
///
/// A run comes from either the command line highlighted as shell or the output
/// parsed as ANSI. `color: None` is the terminal's default foreground, which is
/// drawn in the line kind's own colour rather than a fixed palette entry, so a
/// plain command still reads as the block's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRun {
    pub text: String,
    pub color: Option<ColorU>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A single line inside a [`TerminalBlock`](super::block::TerminalBlock).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalLine {
    pub text: String,
    pub kind: TerminalLineKind,
    /// The line's styled runs, when it carries colour or emphasis. Empty for a
    /// plain line, which is drawn as `text` in the line kind's colour.
    pub runs: Vec<TerminalRun>,
}

impl TerminalLine {
    pub fn command(text: impl Into<String>) -> Self {
        Self::styled(text, TerminalLineKind::Command, Vec::new())
    }

    pub fn output(text: impl Into<String>) -> Self {
        Self::styled(text, TerminalLineKind::Output, Vec::new())
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::styled(text, TerminalLineKind::Info, Vec::new())
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::styled(text, TerminalLineKind::Success, Vec::new())
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::styled(text, TerminalLineKind::Error, Vec::new())
    }

    /// A line with the styled runs a highlighted command or a parsed output
    /// stream produced. `text` stays the line's plain text — copy, filters and
    /// the content key all read it — while `runs` is only what is drawn.
    pub fn styled(text: impl Into<String>, kind: TerminalLineKind, runs: Vec<TerminalRun>) -> Self {
        Self {
            text: text.into(),
            kind,
            runs,
        }
    }
}
