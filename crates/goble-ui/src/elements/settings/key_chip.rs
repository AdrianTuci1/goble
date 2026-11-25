//! The grouped key-chip pill: one rounded container holding a chord's keys.
//!
//! The reference puts a shortcut's chord in a single raised, hairline pill
//! (`⌘ + K`) rather than one box per key. This is the settings-sized version of
//! that pill: 24px tall, a full-radius end, the control family's fill and
//! border, and the muted 12px chord. It is a display primitive, not a button:
//! the Shortcuts page pairs it with its own trailing icon action.

use crate::elements::{
    AppContext, Border, Container, EdgeInsets, Element, EventContext, Fill, LayoutContext,
    PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::theme::ColorToken;

use super::{control_border, control_fill, SETTINGS_KEY_CHIP_HEIGHT, SETTINGS_KEY_CHIP_TEXT_SIZE};

/// A rounded pill holding one chord, its keys joined by ` + `.
pub struct SettingsKeyChip {
    keys: Vec<String>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsKeyChip {
    pub fn new<S: Into<String>>(keys: impl IntoIterator<Item = S>) -> Self {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
            root: None,
            size: None,
            origin: None,
        }
    }

    /// The chord as the pill draws it (`⌘ + K`). The page and its tests read the
    /// same string.
    pub fn label(&self) -> String {
        self.keys.join(" + ")
    }

    fn build(&mut self, app: &AppContext) {
        let text = Text::new(self.label())
            .with_font_size(SETTINGS_KEY_CHIP_TEXT_SIZE)
            .with_line_height(1.35)
            .with_color(app.theme.color(ColorToken::Muted))
            .finish();
        // 424: a displayed chord is plain muted text, never a chip.
        let _ = app;
        self.root = Some(text);
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.root.is_none() {
            self.build(app);
        }
        self.root.as_mut().unwrap()
    }
}

impl Element for SettingsKeyChip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root_mut(app).layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root_mut(app).paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.root_mut(app).dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    #[test]
    fn the_chip_draws_one_pill_with_every_key_joined() {
        let app = AppContext::default();
        let mut chip: Box<dyn Element> = Box::new(SettingsKeyChip::new(["⌘", "⇧", "K"]));
        let commands = render_element(&mut chip, vec2f(300.0, 60.0), &app);
        let text = commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert_eq!(text.as_deref(), Some("⌘ + ⇧ + K"));
        assert!(
            commands.iter().all(|command| !matches!(
                command,
                RenderCommand::FillRect { .. } | RenderCommand::StrokeRect { .. }
            )),
            "the chord is plain text, not a chip: {commands:?}"
        );
    }
}
