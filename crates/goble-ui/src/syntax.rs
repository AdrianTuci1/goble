//! Syntax highlighting for code the transcript shows.
//!
//! One entry point, [`highlight`], turns `(text, language)` into the styled runs
//! the renderer already paints ([`TextSpan`]) — there is no second text model.
//! `language` is a markdown fence info string, a language token, a file
//! extension or a file path, so a fenced block, a file read and a command all
//! resolve through the same call.
//!
//! The grammars are bat's, through `two-face`, and the regex engine is syntect's
//! Oniguruma backend — about 2.6x faster than the pure-Rust one on a large file,
//! which is what keeps a review's colouring off the frame. The theme is
//! grok-build's own Grok Night, extracted from the harness and loaded from
//! `assets/themes`, so the same file reads alike in both. Only syntect's
//! foreground survives — its background is dropped, the way grok-build's
//! `syntect_to_ratatui_fg` keeps the colour and the modifiers and nothing else.

use std::io::Cursor;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::color::ColorU;
use crate::elements::TextSpan;
use crate::platform::text_atlas::FontWeight;
use crate::theme::FontFamily;

/// GrokBuild's own night theme, byte for byte, so a file coloured here and the
/// same file coloured in the harness read alike. The grammars are bat's and the
/// engine is syntect, which is what the harness uses too.
const THEME_FILE: &[u8] = include_bytes!("../assets/themes/grok-night.tmTheme");

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
            .expect("the bundled Grok Night theme must parse");
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

/// One highlighted run without the render-only callback a [`TextSpan`] carries.
/// A [`TextSpan`] is not `Send`, so a background highlighter produces these and
/// the UI thread turns them into spans with [`spans_to_lines`]. Everything here
/// is plain data, which is what lets a large file be coloured off the frame
/// that reads it.
#[derive(Clone, Debug)]
pub struct HighlightSpan {
    pub text: String,
    pub color: ColorU,
    pub bold: bool,
    pub italic: bool,
}

/// A whole file's runs, line by line, in the form a worker can send back.
pub type HighlightedSpans = Vec<Vec<HighlightSpan>>;

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
    highlight_spans(text, language).map(|lines| spans_to_lines(&lines))
}

/// The same colouring as [`highlight`], in a form a worker thread can build and
/// send: no callbacks, no `Rc`. A host that reads a large file off the frame
/// uses this and converts the answer with [`spans_to_lines`].
pub fn highlight_spans(text: &str, language: &str) -> Option<HighlightedSpans> {
    let syntaxes = syntaxes();
    let token = language_token(language)?;
    let syntax = syntaxes.syntax_set.find_syntax_by_token(token)?;
    let mut highlighter = HighlightLines::new(syntax, &syntaxes.theme);
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(text) {
        let styled = highlighter
            .highlight_line(line, &syntaxes.syntax_set)
            .ok()?;
        let mut spans = Vec::new();
        for (style, text) in styled {
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                continue;
            }
            spans.push(HighlightSpan {
                text: text.to_string(),
                color: ColorU::new(
                    style.foreground.r,
                    style.foreground.g,
                    style.foreground.b,
                    style.foreground.a,
                ),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            });
        }
        lines.push(spans);
    }
    Some(lines)
}

/// Turn a worker's plain runs into the spans the text model paints.
pub fn spans_to_lines(spans: &HighlightedSpans) -> Vec<HighlightedLine> {
    spans
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| {
                    let mut out = TextSpan::plain(span.text.clone())
                        .with_family(FontFamily::Mono)
                        .with_color(span.color);
                    if span.bold {
                        out = out.with_weight(FontWeight::Bold);
                    }
                    if span.italic {
                        out = out.with_italic(true);
                    }
                    out
                })
                .collect()
        })
        .collect()
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
        // The app's own configuration file is opened in a pane and drawn from
        // this call, so its path has to resolve like any other.
        let config = highlight("model = \"gpt-4o\"\n", "~/.goble/config.toml")
            .expect("config.toml resolves as TOML");
        let colours: HashSet<ColorU> = colours(&config);
        assert!(
            colours.len() > 1,
            "a TOML key and its string are not one colour, got {colours:?}"
        );
    }

    #[test]
    fn a_fence_info_string_resolves_its_first_token() {
        assert!(highlight("fn main() {}", "rust,ignore").is_some());
    }

    /// The harness's theme, not a lookalike: a doc comment, a keyword and a
    /// string come out in Grok Night's own colours, so a file read here and the
    /// same file read in grok-build agree.
    #[test]
    fn the_theme_is_grok_builds_own_night_theme() {
        let lines = highlight("/// note\nfn main() { let s = \"hi\"; }", "rust")
            .expect("rust must resolve");
        let spans: Vec<&TextSpan> = lines.iter().flatten().collect();
        let colour_of = |text: &str| {
            spans
                .iter()
                .find(|span| span.text == text)
                .map(|span| span.color)
        };
        // The doc comment is split into `///` and ` note`; both are its colour.
        let comment: Vec<&TextSpan> = spans
            .iter()
            .copied()
            .filter(|span| span.text.starts_with("///") || span.text == " note")
            .collect();
        assert!(!comment.is_empty(), "the doc comment is drawn");
        assert!(
            comment
                .iter()
                .all(|span| span.color == ColorU::new(0x51, 0x59, 0x7d, 0xff)),
            "the doc comment wears Grok Night's comment colour"
        );
        assert_eq!(
            colour_of("fn"),
            Some(ColorU::new(0xbb, 0x9a, 0xf7, 0xff)),
            "the function keyword wears its keyword colour"
        );
        assert_eq!(
            colour_of("hi"),
            Some(ColorU::new(0x9e, 0xce, 0x6a, 0xff)),
            "the string's text wears its string colour"
        );
    }
}
