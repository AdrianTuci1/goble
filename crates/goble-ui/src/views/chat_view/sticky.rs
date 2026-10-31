//! The sticky turn header: the latest user message pinned to the top of the
//! transcript while its own bubble has scrolled above the viewport's top edge.
//!
//! The row is grok-build's sticky turn, not a header for the whole scroll: it
//! shows while the user's bubble is passing the top edge and is released as
//! soon as the reply under it reaches that edge, so the pin never outlives the
//! gap between the two messages it names.
//!
//! The tree is rebuilt every frame and no message stores its own geometry, so
//! the two rows the pin reads are wrapped in [`RowProbe`]s: each records where
//! it was painted (window coordinates, the transcript's own scroll already
//! applied) and the header, painted after the transcript in the same frame,
//! decides from those recordings. That is the transcript's row recording, not
//! a second measurement pass over every message.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::ChatFragmentKind;
use crate::elements::text::measure_text;
use crate::elements::{
    AppContext, ChatMessage, ChatRole, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};

/// The pinned row's one line of text.
const TEXT_FONT_SIZE: f32 = 12.0;
const TEXT_LINE_HEIGHT: f32 = 1.2;

/// The rule under the pinned row, in points.
const DIVIDER_HEIGHT: f32 = 1.0;

/// How far the user's bubble must have crossed the viewport's top edge before
/// the pin shows. Without it a bubble whose top sits exactly on the edge —
/// the transcript at its top — would pin itself while it is still visible.
const EDGE_EPSILON: f32 = 0.5;

/// Where one transcript row was painted, in window coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RowGeometry {
    top: f32,
    height: f32,
}

/// The slot a probe writes its geometry into, read by the header.
type Slot = Rc<RefCell<Option<RowGeometry>>>;

/// Wraps one transcript row and records where it was painted. The wrapper
/// draws nothing and forwards layout, paint and events unchanged, like the
/// selection surface does for text.
struct RowProbe {
    child: Box<dyn Element>,
    slot: Slot,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RowProbe {
    fn new(child: Box<dyn Element>, slot: Slot) -> Self {
        Self {
            child,
            slot,
            size: None,
            origin: None,
        }
    }
}

impl Element for RowProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(size) = self.size {
            *self.slot.borrow_mut() = Some(RowGeometry {
                top: origin.y,
                height: size.y,
            });
        }
        self.child.paint(origin, ctx, app);
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
        self.child.dispatch_event(event, ctx, app)
    }
}

/// Which rows the pin reads, and what it says: the last user message and the
/// message after it. `text` is `None` when there is nothing to pin, in which
/// case no probe is wrapped and no header is drawn.
pub(super) struct StickyTurn {
    text: Option<String>,
    user_index: Option<usize>,
    next_index: Option<usize>,
    user: Slot,
    next: Slot,
}

impl StickyTurn {
    /// Read the transcript for the one row that can pin. Only the latest user
    /// message pins, and only when another message follows it: with no reply
    /// to release the pin on, the row would stay for the whole scroll.
    pub(super) fn for_messages(messages: &[ChatMessage]) -> Self {
        let mut sticky = Self {
            text: None,
            user_index: None,
            next_index: None,
            user: Slot::default(),
            next: Slot::default(),
        };
        let Some(user_index) = messages.iter().rposition(|m| m.role == ChatRole::User) else {
            return sticky;
        };
        let Some(text) = first_line(&messages[user_index]) else {
            return sticky;
        };
        let next_index = user_index + 1;
        if messages.get(next_index).is_none() {
            return sticky;
        }
        sticky.text = Some(text);
        sticky.user_index = Some(user_index);
        sticky.next_index = Some(next_index);
        sticky
    }

    /// Wrap `child` when it is one of the two rows the pin reads.
    pub(super) fn probe(&self, index: usize, child: Box<dyn Element>) -> Box<dyn Element> {
        if self.user_index == Some(index) {
            RowProbe::new(child, self.user.clone()).finish()
        } else if self.next_index == Some(index) {
            RowProbe::new(child, self.next.clone()).finish()
        } else {
            child
        }
    }

    /// The overlay that draws the pinned row, or `None` when this transcript
    /// has nothing to pin.
    pub(super) fn overlay(self) -> Option<Box<dyn Element>> {
        Some(
            StickyHeader {
                text: self.text?,
                user: self.user,
                next: self.next,
                size: None,
                origin: None,
            }
            .finish(),
        )
    }
}

/// The pinned row itself: the transcript background over the viewport's top
/// strip, one line of the user's message and a rule under it.
struct StickyHeader {
    text: String,
    user: Slot,
    next: Slot,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl StickyHeader {
    /// The strip's height: the text's line box and the inset over and under it.
    fn height(app: &AppContext) -> f32 {
        TEXT_FONT_SIZE * TEXT_LINE_HEIGHT + 2.0 * app.theme.spacing_px(SpacingToken::Xs)
    }
}

impl Element for StickyHeader {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The overlay spans the transcript's width and takes only its own strip
        // of height: it is laid out as a child of the transcript's stack, which
        // must not grow to the row.
        let width = if constraint.max.x.is_finite() {
            constraint.max.x.max(constraint.min.x)
        } else {
            constraint.min.x
        };
        let size = vec2f(width, Self::height(app));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else {
            return;
        };
        if size.x <= 0.0 {
            return;
        }
        let (Some(user), Some(next)) = (*self.user.borrow(), *self.next.borrow()) else {
            return;
        };
        // The pin is on only while the user's bubble has crossed the top edge
        // and the reply under it has not reached that edge yet.
        let top = origin.y;
        if user.top + EDGE_EPSILON >= top || next.height <= 0.0 || next.top <= top {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let pad_x = app.theme.spacing_px(SpacingToken::Md);
        renderer.fill_rect(
            rectf(origin.x, top, size.x, size.y),
            app.theme.color(ColorToken::Bg),
        );
        let available = (size.x - 2.0 * pad_x).max(0.0);
        if available > 0.0 {
            let line = elide_to_width(&self.text, available);
            renderer.draw_text_with_font(
                vec2f(
                    origin.x + pad_x,
                    top + (size.y - TEXT_FONT_SIZE * TEXT_LINE_HEIGHT) / 2.0,
                ),
                line,
                TEXT_FONT_SIZE,
                app.theme.color(ColorToken::Text),
                available,
                TEXT_LINE_HEIGHT,
                FontWeight::Regular,
                FontFamily::System,
                false,
            );
        }
        renderer.fill_rect(
            rectf(
                origin.x,
                top + size.y - DIVIDER_HEIGHT,
                size.x,
                DIVIDER_HEIGHT,
            ),
            app.theme.color(ColorToken::Border),
        );
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The first line of a message's own text: the runs up to the first line break,
/// with a block that opens a line of its own ending the read. A message with no
/// text of its own (only tool calls, say) has nothing to pin.
fn first_line(message: &ChatMessage) -> Option<String> {
    let mut line = String::new();
    for fragment in &message.fragments {
        let text = match &fragment.kind {
            ChatFragmentKind::Text(text)
            | ChatFragmentKind::Bold(text)
            | ChatFragmentKind::Italic(text)
            | ChatFragmentKind::BoldItalic(text)
            | ChatFragmentKind::Code(text) => text.as_str(),
            ChatFragmentKind::Heading { text, .. } => text.as_str(),
            ChatFragmentKind::Link { label, .. } => label.as_str(),
            ChatFragmentKind::LineBreak
            | ChatFragmentKind::CodeBlock { .. }
            | ChatFragmentKind::List { .. }
            | ChatFragmentKind::BlockQuote(_)
            | ChatFragmentKind::Table { .. }
            | ChatFragmentKind::Rule
            | ChatFragmentKind::Image { .. }
            | ChatFragmentKind::Action { .. }
            | ChatFragmentKind::Terminal(_)
            | ChatFragmentKind::Reasoning { .. } => break,
        };
        if let Some((first, _)) = text.split_once('\n') {
            line.push_str(first);
            break;
        }
        line.push_str(text);
    }
    let line = line.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// `text` cut with an ellipsis so it measures at most `max_width` in the pinned
/// row's font. The row is one line, so the cut is what keeps a long message
/// from drawing past the pane.
fn elide_to_width(text: &str, max_width: f32) -> String {
    let width = |candidate: &str| {
        measure_text(candidate, TEXT_FONT_SIZE, TEXT_LINE_HEIGHT, f32::INFINITY).x
    };
    if width(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut low = 0usize;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high + 1) / 2;
        let candidate: String = chars[..mid].iter().chain(std::iter::once(&'…')).collect();
        if width(&candidate) <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    chars[..low].iter().chain(std::iter::once(&'…')).collect()
}

/// The rect of the pinned strip in a frame's commands, when one was drawn: a
/// transcript-background fill across the pane at the top of the transcript.
#[cfg(test)]
pub(super) fn painted_strip(
    commands: &[crate::render::RenderCommand],
    app: &AppContext,
    width: f32,
) -> Option<crate::geometry::RectF> {
    let bg = app.theme.color(ColorToken::Bg);
    let height = StickyHeader::height(app);
    commands.iter().find_map(|command| match command {
        crate::render::RenderCommand::FillRect { rect, color, .. }
            if *color == bg
                && (rect.width() - width).abs() < 0.01
                && (rect.height() - height).abs() < 0.01
                && rect.min_y().abs() < 0.01 =>
        {
            Some(*rect)
        }
        _ => None,
    })
}
