//! The sticky turn header: the latest user message pinned to the top of the
//! transcript while its own bubble has scrolled above the viewport's top edge.
//!
//! The row is grok-build's sticky turn, not a header for the whole scroll: it
//! shows while the user's bubble is passing the top edge and stays for the
//! whole turn that bubble opens — through every assistant row and tool row —
//! until the next user prompt reaches that edge. The pin names a turn, so it
//! must not give way at the first reply under it.
//!
//! The pinned row is the user's own prompt band, not a header of its own: the
//! same grey fill, the same `❯` marker at the same offset and the same 14 px
//! text as the bubble it pins, so the strip reads as that band's top edge while
//! the rest of the band scrolls underneath.
//!
//! The tree is rebuilt every frame and no message stores its own geometry, so
//! the rows the pin reads — the prompt it names, and the next prompt when one
//! follows — are wrapped in [`RowProbe`]s: each records where it was painted
//! (window coordinates, the transcript's own scroll already applied) and the
//! header, painted after the transcript in the same frame, decides from those
//! recordings. That is the transcript's row recording, not a second measurement
//! pass over every message.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::ChatFragmentKind;
use crate::elements::chat_typography;
use crate::elements::{
    AppContext, ChatMessage, ChatRole, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{advance_width, measure_text_family, FontWeight};
use crate::theme::ColorToken;

/// The paragraph size of the pinned row. It mirrors the constants of
/// [`ChatMessageBubble`](crate::elements::ChatMessageBubble)'s user row so the
/// pinned line is the size and shape of the message it pins; the sticky turn's
/// tests compare the pin against that row's own render commands, which is what
/// keeps the two in step.
const MESSAGE_FONT_SIZE: f32 = chat_typography::BODY_SIZE;
const MESSAGE_LINE_HEIGHT: f32 = chat_typography::USER_LH;

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
/// next user message after it. `text` is `None` when there is nothing to pin,
/// in which case no probe is wrapped and no header is drawn.
pub(super) struct StickyTurn {
    text: Option<String>,
    user_index: Option<usize>,
    next_index: Option<usize>,
    user: Slot,
    next: Slot,
}

impl StickyTurn {
    /// Read the transcript for the one row that can pin. Only the latest user
    /// message pins. The pin gives way at the next user prompt, not at the
    /// first row under it: every assistant row and tool row between the two
    /// belongs to the turn the header names, so the header stays while that
    /// turn is being read. A transcript with no later prompt has no release
    /// anchor and keeps the pin for the whole scroll.
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
        let next_index = messages
            .iter()
            .enumerate()
            .skip(user_index + 1)
            .find_map(|(index, message)| (message.role == ChatRole::User).then_some(index));
        sticky.text = Some(text);
        sticky.user_index = Some(user_index);
        sticky.next_index = next_index;
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

/// The pinned row itself: the first line of the user's own prompt band over the
/// viewport's top strip, with the rule under it that separates the band from
/// the content scrolled beneath.
struct StickyHeader {
    text: String,
    user: Slot,
    next: Slot,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl StickyHeader {
    /// The strip's height: one line of the bubble's padded band — the marker's
    /// column and the message's first line, inset by `Md` over and under it.
    fn height(_app: &AppContext) -> f32 {
        MESSAGE_FONT_SIZE * MESSAGE_LINE_HEIGHT + 2.0 * chat_typography::BAND_PADDING_Y
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
        let Some(user) = *self.user.borrow() else {
            return;
        };
        // The pin is on while the user's bubble has crossed the top edge, and
        // off again once the next prompt reaches that edge: the rows between
        // the two are this turn's own replies and tool calls, and they leave
        // the pin in place. With no later prompt the row stays for the whole
        // scroll.
        let top = origin.y;
        let next = *self.next.borrow();
        let next_reached_top = next.is_some_and(|next| next.height > 0.0 && next.top <= top);
        if user.top + EDGE_EPSILON >= top || next_reached_top {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        // The band the user's row draws, its marker and its first line, at the
        // same insets: the strip must read as that band's top edge, not as a
        // surface of its own.
        let pad_x = chat_typography::BAND_PADDING_X;
        let pad_y = chat_typography::BAND_PADDING_Y;
        let family = chat_typography::chat_font(app);
        let cap = chat_typography::user_bubble_max_width(size.x);
        // Match the advance InlineText uses to size the prompt's bubble, not
        // the ink extent (which is a hair narrower than the room the run takes).
        let measured = advance_width(
            &self.text,
            MESSAGE_FONT_SIZE,
            FontWeight::Regular,
            family,
            false,
        );
        // The pin mirrors the prompt's right-aligned bubble: only as wide as
        // its first line (capped like the bubble) and anchored to the lane's
        // trailing edge.
        let band_width = (measured + pad_x * 2.0).min(cap).min(size.x).max(0.0);
        let band_x = origin.x + size.x - band_width;
        // The pinned band mirrors the user's rounded bubble: no prompt marker,
        // so the first line starts at the band's own text inset.
        renderer.fill_rounded_rect(
            rectf(band_x, top, band_width, size.y),
            app.theme.color(ColorToken::SurfaceRaised),
            chat_typography::USER_BUBBLE_RADIUS,
        );
        let text_x = band_x + pad_x;
        let available = (band_width - pad_x * 2.0).max(0.0);
        if available > 0.0 {
            let line = elide_to_width(&self.text, available, family);
            renderer.draw_text_with_font(
                vec2f(text_x, top + pad_y),
                line,
                MESSAGE_FONT_SIZE,
                app.theme.color(ColorToken::Text),
                available,
                MESSAGE_LINE_HEIGHT,
                FontWeight::Regular,
                family,
                false,
            );
        }
        renderer.fill_rect(
            rectf(
                band_x,
                top + size.y - DIVIDER_HEIGHT,
                band_width,
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
            | ChatFragmentKind::Ansi(_)
            | ChatFragmentKind::List { .. }
            | ChatFragmentKind::BlockQuote(_)
            | ChatFragmentKind::Table { .. }
            | ChatFragmentKind::Rule
            | ChatFragmentKind::Image { .. }
            | ChatFragmentKind::Action { .. }
            | ChatFragmentKind::Terminal(_)
            | ChatFragmentKind::Notice(_)
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
fn elide_to_width(text: &str, max_width: f32, family: crate::theme::FontFamily) -> String {
    let width = |candidate: &str| {
        measure_text_family(
            candidate,
            MESSAGE_FONT_SIZE,
            MESSAGE_LINE_HEIGHT,
            f32::INFINITY,
            FontWeight::Regular,
            family,
            false,
        )
        .x
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

/// The rect of the pinned strip in a frame's commands, when one was drawn: the
/// user's grey bubble anchored to the pane's trailing edge at the top of the
/// transcript, with the rule the pin draws under it.
///
/// The fill alone is not the pin's signature: a user row draws the same fill at
/// the same trailing edge, and a one-line message's band is exactly the strip's
/// height. The rule inside the band's bottom edge is what tells the two apart —
/// no transcript row draws a border.
#[cfg(test)]
pub(super) fn painted_strip(
    commands: &[crate::render::RenderCommand],
    app: &AppContext,
    width: f32,
) -> Option<crate::geometry::RectF> {
    use crate::render::RenderCommand;

    let band = app.theme.color(ColorToken::SurfaceRaised);
    let rule = app.theme.color(ColorToken::Border);
    let height = StickyHeader::height(app);
    let right_aligned = |rect: &crate::geometry::RectF| {
        (rect.max_x() - width).abs() < 0.01 && rect.width() <= width + 0.01
    };
    commands.iter().enumerate().find_map(|(index, command)| {
        let RenderCommand::FillRect { rect, color, .. } = command else {
            return None;
        };
        if *color != band
            || !right_aligned(rect)
            || (rect.height() - height).abs() >= 0.01
            || rect.min_y().abs() >= 0.01
        {
            return None;
        }
        let edge = commands[index + 1..].iter().any(|later| {
            matches!(later,
                RenderCommand::FillRect { rect: line, color, .. }
                    if *color == rule
                        && (line.width() - rect.width()).abs() < 0.01
                        && (line.max_x() - rect.max_x()).abs() < 0.01
                        && (line.height() - DIVIDER_HEIGHT).abs() < 0.01
                        && (line.min_y() - (rect.max_y() - DIVIDER_HEIGHT)).abs() < 0.01)
        });
        edge.then_some(*rect)
    })
}
