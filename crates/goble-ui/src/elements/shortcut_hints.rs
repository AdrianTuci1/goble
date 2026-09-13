//! The rich input's instruction strip: one entry per gesture, each a key cap
//! and the name of what the key does.
//!
//! The shape is the warp-new shortcuts view: a cap per key of the chord (two
//! caps for `⌘K`), then the name in the muted weight. The entries are the
//! caller's — the surface that owns the bindings owns the list, so the strip
//! never names a key nothing answers.
//!
//! The strip wraps rather than overflows: entries are whole items, so in a
//! narrow pane the instructions run onto a second line instead of being cut off
//! at the pane's edge, which is what warp-new's input footer does at resize.

use crate::elements::{
    AppContext, Border, Container, CrossAxisAlignment, EdgeInsets, Element, Empty, Fill, Flex, Icon,
    Text, Wrap,
};
use crate::theme::{ColorToken, SpacingToken};

/// One entry: the keys of a gesture and the name of what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutHint {
    keys: Vec<String>,
    label: String,
}

impl ShortcutHint {
    /// `keys` is the chord, one cap each: `["⌘", "K"]` draws two caps and a
    /// single-key gesture draws one. `label` is the short name of what the key
    /// does, in the same weight as the rest of the strip.
    pub fn new(keys: &[&str], label: impl Into<String>) -> Self {
        Self {
            keys: keys.iter().map(|key| (*key).to_string()).collect(),
            label: label.into(),
        }
    }

    /// The keys, one per cap.
    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// The name drawn after the caps.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The chord's caps on their own, without the name: the key column of a
    /// list whose names line up down the rows instead of following each chord
    /// inline. The caps are the strip's own, so a chord looks the same in both.
    pub fn caps(&self, app: &AppContext) -> Box<dyn Element> {
        let mut caps = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(2.0);
        for key in &self.keys {
            caps = caps.with_child(key_cap(key, app));
        }
        caps.finish()
    }
}

/// The strip: `ShortcutHint` entries left to right, one weight for all of them.
pub struct ShortcutHints {
    hints: Vec<ShortcutHint>,
}

impl ShortcutHints {
    pub fn new(hints: Vec<ShortcutHint>) -> Self {
        Self { hints }
    }

    /// The composed strip. An empty list is an [`Empty`], so a surface with no
    /// instructions pays neither the strip's height nor the column's gap.
    pub fn finish(self, app: &AppContext) -> Box<dyn Element> {
        if self.hints.is_empty() {
            return Empty::new().finish();
        }
        let mut row = Wrap::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(app.theme.spacing_px(SpacingToken::Md))
            .with_run_spacing(app.theme.spacing_px(SpacingToken::Sm));
        for hint in self.hints {
            row = row.with_child(entry(hint, app));
        }
        row.finish()
    }
}

/// One entry: the caps of the chord, 2px apart, then its name 4px later.
fn entry(hint: ShortcutHint, app: &AppContext) -> Box<dyn Element> {
    let caps = hint.caps(app);
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(caps)
        .with_child(
            Text::new(hint.label)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish(),
        )
        .finish()
}

/// The icon a key cap draws instead of its character.
///
/// The bundled text faces carry none of the modifier symbols a chord is written
/// with — `⌘ ⇧ ⌥ ⌃ ↵ ⌫ ⇥` and the arrow keys are all missing from Roboto and
/// Hack — and an uncovered character rasterizes as the font's `.notdef` box,
/// which reads as a small hatched rectangle instead of a key. Every key whose
/// symbol the fonts lack is therefore drawn from the icon set; only the letters,
/// digits and ASCII keys (`K`, `W`, `!`, `Space`, `Esc`) fall through to text.
fn key_icon(key: &str) -> Option<&'static str> {
    match key {
        "⌘" => Some("key-command"),
        "⇧" => Some("key-shift"),
        "⌥" => Some("key-option"),
        "⌃" => Some("key-control"),
        "↵" | "⏎" => Some("key-return"),
        "⌫" => Some("key-delete"),
        "⇥" => Some("key-tab"),
        "↑" => Some("key-arrow-up"),
        "↓" => Some("key-arrow-down"),
        "←" => Some("key-arrow-left"),
        "→" => Some("key-arrow-right"),
        _ => None,
    }
}

/// Whether a key cap draws this key from the icon set rather than from its
/// character. The callers that assert the cap's ink use it to tell a key that
/// must be drawn as an icon from one the text font can carry.
pub fn key_has_icon(key: &str) -> bool {
    key_icon(key).is_some()
}

/// One key of the chord: a square cap. The surface is flat (no corner radius),
/// like every other control in the workspace.
fn key_cap(key: &str, app: &AppContext) -> Box<dyn Element> {
    let content: Box<dyn Element> = match key_icon(key) {
        Some(name) => Icon::new(name)
            .with_size(11.0)
            .with_theme_color(ColorToken::Text, app)
            .finish(),
        None => Text::new(key.to_string())
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(11.0)
            .with_max_lines(1)
            .finish(),
    };
    Container::new(content)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_border(
            Border::all(1.0).with_border_fill(Fill::Solid(app.theme.color(ColorToken::Border))),
        )
        .with_padding(EdgeInsets::new(6.0, 1.0, 6.0, 1.0))
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    fn app() -> AppContext {
        AppContext::default()
    }

    fn drawn(commands: &[RenderCommand], text: &str) -> Vec<f32> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text: run, origin, .. } if run == text => Some(origin.y),
                _ => None,
            })
            .collect()
    }

    fn at(commands: &[RenderCommand], text: &str) -> (f32, f32) {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text: run, origin, .. } if run == text => {
                    Some((origin.x, origin.y))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text:?} is drawn"))
    }

    fn icons(commands: &[RenderCommand], name: &str) -> Vec<(f32, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name: run, origin, .. } if run == name => {
                    Some((origin.x, origin.y))
                }
                _ => None,
            })
            .collect()
    }

    fn caps(commands: &[RenderCommand]) -> usize {
        commands
            .iter()
            .filter(|command| {
                matches!(command, RenderCommand::StrokeRect { rect, corner_radius, .. }
                    if *corner_radius == 0.0 && rect.height() < 20.0)
            })
            .count()
    }

    /// Every key of a chord is its own cap, the name follows it, and the row
    /// paints no rounded corner. A key whose symbol the bundled text faces lack
    /// is drawn from the icon set instead — as an icon and no character — while
    /// an ASCII key keeps its own glyph.
    #[test]
    fn the_strip_draws_a_cap_per_key_and_the_name() {
        let app = app();
        let hints = ShortcutHints::new(vec![
            ShortcutHint::new(&["↵"], "send"),
            ShortcutHint::new(&["⌘", "⇧", "W"], "tasks"),
        ]);
        let mut element = hints.finish(&app);
        let commands = render_element(&mut element, vec2f(600.0, 40.0), &app);

        // Four caps for the two entries: one for `↵`, three for `⌘⇧W`.
        assert_eq!(caps(&commands), 4, "one cap per key of every chord");
        for (key, icon) in [
            ("⌘", "key-command"),
            ("⇧", "key-shift"),
            ("↵", "key-return"),
        ] {
            assert!(
                !icons(&commands, icon).is_empty(),
                "the {key:?} cap is the {icon} icon: {commands:?}"
            );
            assert!(
                drawn(&commands, key).is_empty(),
                "and it draws no character for {key:?}"
            );
        }
        assert!(
            !drawn(&commands, "W").is_empty(),
            "a key the text font carries keeps its own glyph"
        );
        for label in ["send", "tasks"] {
            assert!(!drawn(&commands, label).is_empty(), "the name {label:?} is drawn");
        }
        // The name sits on the caps' own line, to their right.
        let label_x = at(&commands, "send").0;
        let icon_x = icons(&commands, "key-return")[0].0;
        assert!(label_x > icon_x, "the name follows its caps");
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                RenderCommand::FillRect { corner_radius, .. } if *corner_radius > 0.0
            )),
            "the strip is flat"
        );
    }

    /// The caps never draw a character their font lacks: every key the icon set
    /// answers is drawn as an icon, and the rest are plain ASCII the bundled
    /// faces cover.
    #[test]
    fn a_cap_draws_a_symbol_the_text_font_lacks_as_an_icon() {
        use crate::platform::text_atlas::{font_covers, FontWeight};
        use crate::theme::FontFamily;

        for key in ["⌘", "⇧", "⌥", "⌃", "↵", "⏎", "⌫", "⇥", "↑", "↓", "←", "→"] {
            assert!(key_has_icon(key), "{key:?} is drawn from the icon set");
        }
        for key in ["K", "W", "N", "Space", "Esc", "!"] {
            assert!(!key_has_icon(key), "{key:?} is a text cap");
            assert!(
                font_covers(key, FontFamily::System, FontWeight::Regular),
                "and the bundled face covers {key:?}"
            );
        }
    }

    /// A chord's caps can be drawn without its name: a list whose names line up
    /// down the rows puts each chord's caps in its own column. The caps are the
    /// strip's own, so the keys look the same in both, and the name of the hint
    /// is not drawn by `caps`.
    #[test]
    fn a_chords_caps_can_be_drawn_without_its_name() {
        let app = app();
        let hint = ShortcutHint::new(&["⌘", "⇧", "W"], "tasks");
        let mut element = hint.caps(&app);
        let commands = render_element(&mut element, vec2f(300.0, 40.0), &app);

        assert_eq!(caps(&commands), 3, "one cap per key of the chord");
        for icon in ["key-command", "key-shift"] {
            assert!(
                !icons(&commands, icon).is_empty(),
                "the {icon} cap is drawn"
            );
        }
        assert!(!drawn(&commands, "W").is_empty(), "so is the text cap");
        assert!(
            drawn(&commands, "tasks").is_empty(),
            "the name is the caller's to draw, not the caps'"
        );
    }

    /// A surface with no instructions pays nothing: the strip is empty, not a
    /// blank row.
    #[test]
    fn an_empty_strip_draws_nothing() {
        let app = app();
        let mut element = ShortcutHints::new(Vec::new()).finish(&app);
        let commands = render_element(&mut element, vec2f(600.0, 40.0), &app);
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                RenderCommand::DrawText { .. } | RenderCommand::FillRect { .. }
            )),
            "an empty strip draws no text and no cap: {commands:?}"
        );
    }

    /// Narrowed, whole entries move to the next line instead of being drawn
    /// past the pane's edge.
    #[test]
    fn the_strip_wraps_instead_of_overflowing() {
        let app = app();
        let hints = || {
            ShortcutHints::new(vec![
                ShortcutHint::new(&["↵"], "send"),
                ShortcutHint::new(&["⌘", "↵"], "new conversation"),
                ShortcutHint::new(&["!"], "shell"),
            ])
        };
        let mut wide = hints().finish(&app);
        let wide_commands = render_element(&mut wide, vec2f(900.0, 80.0), &app);
        assert_eq!(
            at(&wide_commands, "send").1,
            at(&wide_commands, "shell").1,
            "900px is wide enough for one line"
        );

        let width = 200.0;
        let mut narrow = hints().finish(&app);
        let narrow_commands = render_element(&mut narrow, vec2f(width, 80.0), &app);
        assert!(
            at(&narrow_commands, "shell").1 > at(&narrow_commands, "send").1,
            "the entry that no longer fits starts a new line"
        );
        for text in ["send", "new conversation", "shell"] {
            let (x, _) = at(&narrow_commands, text);
            assert!(x < width, "{text:?} stays inside the pane at x={x}");
        }
        for command in &narrow_commands {
            if let RenderCommand::DrawText { origin, .. } = command {
                assert!(origin.x < width, "nothing is drawn past the edge: {command:?}");
            }
        }
    }
}
