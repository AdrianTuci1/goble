//! Syntax highlighting for code the transcript shows.
//!
//! One entry point, [`highlight`], turns `(text, language)` into the styled runs
//! the renderer already paints ([`TextSpan`]) — there is no second text model.
//! `language` is a markdown fence info string, a language token, a file
//! extension or a file path, so a fenced block, a file read and a command all
//! resolve through the same call.
//!
//! The grammars are bat's, through `two-face`; the regex engine is syntect's
//! pure-Rust `fancy-regex` backend, so the build needs no C toolchain. The theme
//! is ours, authored under `assets/themes` and loaded from there. Only syntect's
//! foreground survives — its background is dropped, the way grok-build's
//! `syntect_to_ratatui_fg` keeps the colour and the modifiers and nothing else.

use std::io::Cursor;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::color::ColorU;
use crate::elements::TextSpan;
use crate::platform::text_atlas::FontWeight;
use crate::theme::FontFamily;

/// Our own theme, loaded from the crate's asset directory. It was authored here
/// in grok-build's visual idiom; no theme or grammar file is taken from a
/// reference tree.
const THEME_FILE: &[u8] = include_bytes!("../assets/themes/goble-night.tmTheme");

struct Syntaxes {
    syntax_set: SyntaxSet,
    theme: Theme,
}

/// The grammar set and theme, built once per process. Loading bat's grammar
/// dump is the expensive part, so it is shared rather than rebuilt per frame.
fn syntaxes() -> &'static Syntaxes {
    static SYNTAXES: OnceLock<Syntaxes> = OnceLock::new();
    SYNTAXES.get_or_init(|| {
        let theme = ThemeSet::load_from_reader(&mut Cursor::new(THEME_FILE))
            .expect("the bundled Goble Night theme must parse");
        Syntaxes {
            syntax_set: two_face::syntax::extra_newlines(),
            theme,
        }
    })
}

/// One highlighted line: the styled runs that compose it. It is the same
/// `Vec<TextSpan>` a paragraph is painted from, so code and prose share one
/// text model.
pub type HighlightedLine = Vec<TextSpan>;

/// Turn `text` into styled runs for `language` — the one entry point for code
/// colouring.
///
/// `language` may be a markdown fence info string (`"rust"`, `"rust,ignore"`), a
/// language token (`"bash"`), a file extension (`"rs"`, `"sh"`) or a file path
/// (`"src/main.rs"`, which resolves by its extension).
///
/// Returns `None`, rather than a partial or panicking result, when the language
/// cannot be resolved or the highlighter fails, so the caller keeps drawing the
/// text plain. Every run is mono and carries a foreground colour plus
/// bold/italic; syntect's background is dropped.
pub fn highlight(text: &str, language: &str) -> Option<Vec<HighlightedLine>> {
    let syntaxes = syntaxes();
    let token = language_token(language)?;
    let syntax = syntaxes.syntax_set.find_syntax_by_token(token)?;
    let mut highlighter = HighlightLines::new(syntax, &syntaxes.theme);
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(text) {
        let styled = highlighter
            .highlight_line(line, &syntaxes.syntax_set)
            .ok()?;
        lines.push(styled.into_iter().filter_map(to_text_span).collect());
    }
    Some(lines)
}

/// The lookup key a language string resolves through. A fence info string may
/// carry extra comma- or space-separated words (`rust,ignore`); a path is looked
/// up by its extension.
fn language_token(language: &str) -> Option<&str> {
    let token = language
        .trim()
        .split(|c: char| c.is_whitespace() || c == ',')
        .next()
        .unwrap_or("");
    if token.is_empty() {
        return None;
    }
    if token.contains('/') || token.contains('.') {
        if let Some(extension) = std::path::Path::new(token)
            .extension()
            .and_then(|e| e.to_str())
        {
            return Some(extension);
        }
    }
    Some(token)
}

/// A syntect styled segment as a renderable run: colour, weight and slant only.
fn to_text_span((style, text): (Style, &str)) -> Option<TextSpan> {
    // syntect leaves the line ending on the last run; a run is painted inside
    // its own line, so it is dropped here.
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    if text.is_empty() {
        return None;
    }
    let color = ColorU::new(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
        style.foreground.a,
    );
    let mut span = TextSpan::plain(text)
        .with_family(FontFamily::Mono)
        .with_color(color);
    if style.font_style.contains(FontStyle::BOLD) {
        span = span.with_weight(FontWeight::Bold);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        span = span.with_italic(true);
    }
    // style.background is deliberately not carried: TextSpan keeps its default
    // (no background), which is the "drop syntect's backgrounds" rule.
    Some(span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn colours(lines: &[HighlightedLine]) -> HashSet<ColorU> {
        lines.iter().flatten().map(|span| span.color).collect()
    }

    #[test]
    fn a_rust_snippet_comes_out_multi_coloured() {
        let lines = highlight("fn main() {\n    let x = 1;\n}", "rust").expect("rust must resolve");
        let colours = colours(&lines);
        assert!(
            colours.len() > 1,
            "a Rust snippet must be more than one colour, got {colours:?}"
        );
        assert!(
            lines
                .iter()
                .flatten()
                .all(|span| span.family == FontFamily::Mono),
            "every highlighted run must be mono"
        );
        assert!(
            lines.iter().flatten().all(|span| span.background.is_none()),
            "syntect's background must be dropped"
        );
    }

    #[test]
    fn a_shell_command_comes_out_multi_coloured() {
        let lines =
            highlight("cargo test -p goble-ui -- --nocapture", "bash").expect("bash must resolve");
        let colours = colours(&lines);
        assert!(
            colours.len() > 1,
            "a shell command must be more than one colour, got {colours:?}"
        );
    }

    #[test]
    fn an_unknown_language_falls_back_to_plain() {
        assert!(highlight("fn main() {\n    let x = 1;\n}", "not-a-language").is_none());
        assert!(highlight("fn main() {}", "").is_none());
    }

    #[test]
    fn a_file_path_resolves_by_its_extension() {
        assert!(highlight("fn main() {}", "src/main.rs").is_some());
        assert!(highlight("set -e", "deploy/run.sh").is_some());
    }

    #[test]
    fn a_fence_info_string_resolves_its_first_token() {
        assert!(highlight("fn main() {}", "rust,ignore").is_some());
    }
}
