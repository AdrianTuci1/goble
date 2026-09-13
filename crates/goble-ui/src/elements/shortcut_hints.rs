//! The rich input's instruction strip: one entry per gesture, each a key cap
//! and the name of what the key does.
//!
//! The shape is the warp-new shortcuts view: a cap per key of the chord (two
//! caps for `⌘K`), then the name in the muted weight. The entries are the
//! caller's — the surface that owns the bindings owns the list, so the strip
//! never names a key nothing answers.

use crate::elements::{
    AppContext, Border, Container, CrossAxisAlignment, EdgeInsets, Element, Empty, Fill, Flex, Text,
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
}

/// The strip: `ShortcutHint` entries left to right, one weight for all of them.
pub struct ShortcutHints {
    hints: Vec<ShortcutHint>,
}

impl ShortcutHints {
    pub fn new(hints: Vec<ShortcutHint>) -> Self {
        Self { hints }
    }

    /// The composed row. An empty list is an [`Empty`], so a surface with no
    /// instructions pays neither the row's height nor the column's gap.
    pub fn finish(self, app: &AppContext) -> Box<dyn Element> {
        if self.hints.is_empty() {
            return Empty::new().finish();
        }
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(app.theme.spacing_px(SpacingToken::Md));
        for hint in self.hints {
            row = row.with_child(entry(hint, app));
        }
        row.finish()
    }
}

/// One entry: the caps of the chord, 2px apart, then its name 4px later.
fn entry(hint: ShortcutHint, app: &AppContext) -> Box<dyn Element> {
    let mut caps = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(2.0);
    for key in &hint.keys {
        caps = caps.with_child(key_cap(key, app));
    }
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(caps.finish())
        .with_child(
            Text::new(hint.label)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish(),
        )
        .finish()
}

/// One key of the chord: a square cap. The surface is flat (no corner radius),
/// like every other control in the workspace.
fn key_cap(key: &str, app: &AppContext) -> Box<dyn Element> {
    Container::new(
        Text::new(key.to_string())
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(11.0)
            .with_max_lines(1)
            .finish(),
    )
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
    /// paints no rounded corner.
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
        for key in ["↵", "⌘", "⇧", "W"] {
            assert!(!drawn(&commands, key).is_empty(), "the cap {key:?} is drawn");
        }
        for label in ["send", "tasks"] {
            assert!(!drawn(&commands, label).is_empty(), "the name {label:?} is drawn");
        }
        // The name sits on the caps' own line, to their right.
        let label_x = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "send" => Some(origin.x),
                _ => None,
            })
            .expect("the first name is drawn");
        let cap_x = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "↵" => Some(origin.x),
                _ => None,
            })
            .expect("the cap is drawn");
        assert!(label_x > cap_x, "the name follows its caps");
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                RenderCommand::FillRect { corner_radius, .. } if *corner_radius > 0.0
            )),
            "the strip is flat"
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
}
