//! The rich input's instruction row: one fixed-height line of shortcut hints
//! beside the footer's controls.
//!
//! The row is a **reserved** surface, not a content-sized one: a surface that
//! ships hints keeps this row whether a hint is showing or not, so the composer
//! card and the height a shell pane reserves for its pty bar never move when a
//! hint appears, disappears, changes wording, or the pane narrows. It never
//! wraps: an entry that does not fit the row's width is truncated (the last
//! label takes the ellipsis its width allows) and further entries are dropped,
//! so the row is always exactly [`COMPOSER_HINTS_ROW_HEIGHT`] tall.
//!
//! Self-drawn for the same reason the menus are: the row's height is the card's
//! contract, so it is one value, not the tallest child's.

use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, ShortcutHint, SizeConstraint, Text,
};
use crate::geometry::{vec2f, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

use super::COMPOSER_HINTS_ROW_HEIGHT;

/// The gap between a hint's key caps and its label: the shared strip's 4pt.
const CAPS_LABEL_GAP: f32 = 4.0;
/// The narrowest a hint's label may be drawn before the row drops the entry:
/// a cap column with two or three letters beside it still reads as a hint, and
/// the rest of the list is truncated instead of squeezed into unreadable slivers.
const LABEL_MIN_WIDTH: f32 = 24.0;
/// The row's width when the caller measures it with no width at all.
const FALLBACK_WIDTH: f32 = 360.0;

/// One laid-out child of the row (a hint's caps, then its label) and where it
/// sits on the row.
struct Placed {
    element: Box<dyn Element>,
    offset: Vector2F,
}

/// One fixed-height line of [`ShortcutHint`]s, bottom of the composer input.
pub struct ComposerHintsRow {
    hints: Vec<ShortcutHint>,
    children: Vec<Placed>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ComposerHintsRow {
    pub fn new(hints: Vec<ShortcutHint>) -> Self {
        Self {
            hints,
            children: Vec::new(),
            size: None,
            origin: None,
        }
    }
}

impl Element for ComposerHintsRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let available = if constraint.max.x.is_finite() && constraint.max.x > 0.0 {
            constraint.max.x
        } else {
            FALLBACK_WIDTH
        };
        let height = COMPOSER_HINTS_ROW_HEIGHT;
        self.children.clear();
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let mut x = 0.0_f32;
        for hint in &self.hints {
            let mut caps = hint.caps(app);
            let caps_size = caps.layout(SizeConstraint::loose(vec2f(f32::INFINITY, height)), ctx, app);
            // Leave room for a readable label after the caps; a row too narrow
            // for another entry truncates the list rather than wrapping it.
            let entry_min = caps_size.x + CAPS_LABEL_GAP + LABEL_MIN_WIDTH;
            if x > 0.0 && x + entry_min > available {
                break;
            }
            let mut label = Text::new(hint.label().to_string())
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish();
            let remaining = (available - x - caps_size.x - CAPS_LABEL_GAP).max(0.0);
            let label_size = label.layout(SizeConstraint::loose(vec2f(remaining, height)), ctx, app);
            self.children.push(Placed {
                element: caps,
                offset: vec2f(x, ((height - caps_size.y).max(0.0)) / 2.0),
            });
            self.children.push(Placed {
                element: label,
                offset: vec2f(
                    x + caps_size.x + CAPS_LABEL_GAP,
                    ((height - label_size.y).max(0.0)) / 2.0,
                ),
            });
            x += caps_size.x + CAPS_LABEL_GAP + label_size.x + spacing;
            if x >= available {
                break;
            }
        }
        // Content-sized: the row sits beside the footer's other children, so it
        // reports what it drew, clamped to the space its parent offered. Its
        // height is always the reserved constant, never the children's.
        let width = if self.children.is_empty() {
            0.0
        } else {
            (x - spacing).clamp(0.0, available)
        };
        let size = vec2f(width, height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        for placed in &mut self.children {
            placed.element.paint(origin + placed.offset, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &crate::event::DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        // The row is an instruction strip, not a control: a press on it falls
        // through to the composer's own focus handling.
        for placed in &mut self.children {
            if placed.element.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}
