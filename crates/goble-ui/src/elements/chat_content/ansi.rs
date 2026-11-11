//! ANSI-coloured prose for the transcript.
//!
//! A model that wants colour often reaches for the output a terminal would
//! print, as a fenced block of SGR escapes. A code panel is the wrong shape for
//! that: it is prose, and its colour is the point. This module turns such a
//! block into lines of styled runs, so the transcript can draw it as text
//! without the panel.
//!
//! The escape parsing is the terminal's own [`goble_terminal::ansi::render_lines`]
//! pass, so indexed, truecolor and named foregrounds agree with a pane. A
//! model writes the escape either as a real `ESC` or as the textual form it
//! would type in a shell (`\033`, `\x1b`, `\e`); a block is prose, not code, so
//! the textual forms are translated before parsing.

use goble_terminal::{ansi, Palette, ScreenCell, Underline};

use crate::color::ColorU;

/// One run of ANSI-coloured text: the characters that share a foreground and
/// the attributes in force. Backgrounds are dropped: the transcript paints this
/// as prose on its own surface, never as a filled cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnsiRun {
    pub text: String,
    /// `None` keeps the transcript's text colour for a run that set no
    /// foreground (or reset it).
    pub color: Option<ColorU>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

/// Parse an ANSI-coloured blob into lines of styled runs.
///
/// A block with no escape at all still parses: it comes back as one run per
/// line in the transcript's own colour, which is what a caller asks for when it
/// has already decided the block is prose. Empty leading and trailing lines are
/// kept out, so a fence's own blank padding does not open a gap in the message.
pub fn parse(text: &str) -> Vec<Vec<AnsiRun>> {
    let bytes = textual_escapes_to_bytes(text);
    let palette = Palette::xterm();
    let lines: Vec<Vec<AnsiRun>> = ansi::render_lines(&bytes)
        .iter()
        .map(|line| runs_for_line(line, &palette))
        .collect();
    trim_blank_lines(lines)
}

/// Whether `text` carries an escape sequence in either the real or the textual
/// form a model writes. Used to recognise a fence that is coloured output even
/// when it carries no language label.
pub fn has_escape(text: &str) -> bool {
    text.contains('\u{1b}')
        || text.contains("\\033")
        || text.contains("\\x1b")
        || text.contains("\\x1B")
        || text.contains("\\e[")
        || text.contains("\\u001b")
        || text.contains("\\u{1b}")
}

fn runs_for_line(line: &goble_terminal::ScreenLine, palette: &Palette) -> Vec<AnsiRun> {
    let mut runs: Vec<AnsiRun> = Vec::new();
    for cell in &line.cells {
        let Some(text) = cell_text(cell) else {
            continue;
        };
        let color = (!cell.fg.is_default()).then(|| {
            let (r, g, b) = palette.rgb(cell.fg);
            ColorU::new(r, g, b, 255)
        });
        let bold = cell.attrs.bold;
        let italic = cell.attrs.italic;
        let underline = cell.attrs.underline != Underline::None;
        let strikeout = cell.attrs.strikeout;
        if let Some(last) = runs.last_mut() {
            if last.color == color
                && last.bold == bold
                && last.italic == italic
                && last.underline == underline
                && last.strikeout == strikeout
            {
                last.text.push_str(&text);
                continue;
            }
        }
        runs.push(AnsiRun {
            text,
            color,
            bold,
            italic,
            underline,
            strikeout,
        });
    }
    runs
}

/// The characters a cell paints: its own glyph plus any combining marks, and
/// nothing for the right half of a double-width glyph (the left half carries
/// the character).
fn cell_text(cell: &ScreenCell) -> Option<String> {
    if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
        return None;
    }
    let mut text = String::new();
    text.push(cell.ch);
    text.extend(cell.zerowidth.iter());
    Some(text)
}

fn trim_blank_lines(mut lines: Vec<Vec<AnsiRun>>) -> Vec<Vec<AnsiRun>> {
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

/// Turn the escape spellings a model writes as text into the byte a terminal
/// would receive. A block that already carries `ESC` is left as it is.
fn textual_escapes_to_bytes(text: &str) -> Vec<u8> {
    if !has_escape(text) {
        return text.as_bytes().to_vec();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('\\') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        // Longest spellings first, so `\033` is not left as a stray `\0`.
        // `\e[` spells the `[` too, so it is consumed; the other spellings are
        // just the escape byte and the `[` stays in the remaining text.
        let replaced = [
            ("\\033", ""),
            ("\\x1b", ""),
            ("\\x1B", ""),
            ("\\u001b", ""),
            ("\\u{1b}", ""),
            ("\\e[", "["),
        ]
        .into_iter()
        .find_map(|(spelling, keep)| {
            tail.strip_prefix(spelling).map(|after| {
                out.push('\u{1b}');
                out.push_str(keep);
                after
            })
        });
        match replaced {
            Some(after) => rest = after,
            None => {
                out.push('\\');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str) -> Vec<AnsiRun> {
        parse(text).into_iter().flatten().collect()
    }

    #[test]
    fn a_named_foreground_becomes_a_colour_and_a_reset_clears_it() {
        let runs = runs("\u{1b}[31mred\u{1b}[0m plain");
        assert_eq!(runs[0].text, "red");
        assert_eq!(runs[0].color, Some(ColorU::new(205, 0, 0, 255)));
        assert_eq!(runs[1].text, " plain");
        assert_eq!(runs[1].color, None, "reset returns to the theme's text");
    }

    #[test]
    fn the_textual_escape_a_model_writes_is_understood() {
        let runs = runs("\\033[32mgreen\\033[0m");
        assert_eq!(runs[0].text, "green");
        assert_eq!(runs[0].color, Some(ColorU::new(0, 205, 0, 255)));
    }

    #[test]
    fn attributes_are_carried_but_backgrounds_are_dropped() {
        let runs = runs("\u{1b}[1;3;4;41mbold\u{1b}[0m");
        assert!(runs[0].bold);
        assert!(runs[0].italic);
        assert!(runs[0].underline);
        assert_eq!(
            runs[0].color, None,
            "a background is not a foreground: the run keeps the text colour"
        );
    }

    #[test]
    fn lines_are_kept_apart_and_the_fences_blank_padding_is_trimmed() {
        let lines = parse("\n\\033[31mone\\033[0m\ntwo\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].text, "one");
        assert_eq!(lines[1][0].text, "two");
    }
}
