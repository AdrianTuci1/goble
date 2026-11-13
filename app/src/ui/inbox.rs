//! The global Inbox: a popover hung from the topbar's inbox control, plus the
//! banner that announces a turn which has just finished.
//!
//! The panel is anchored rather than modal. It opens under the inbox button,
//! right-aligned with it, dims nothing, and leaves the workspace underneath
//! live: a press outside it closes it and still lands where it was aimed, and
//! only the panel's own keys are taken while it is up. `j`/`k` walk the rows,
//! `Enter` jumps to the row's pane, `a` acknowledges the row, `A` acknowledges
//! everything and `Esc` closes the panel. Rows the user has acknowledged stay
//! in the list, drawn muted, until the pane they belong to closes.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use goble_ui::elements::{
    AppContext, Axis, Border, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, EventContext, Expanded, Fill, Flex, HoverRow, Icon, KeyHandler, LayoutContext,
    MainAxisSize, PaintContext, Point, Scrollable, SizeConstraint, Spacer, Text, TopbarButton,
};
use goble_ui::event::{DispatchedEvent, ModifiersState, BUTTON_PRIMARY};
use goble_ui::geometry::{vec2f, PointF, RectF, Size2F, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::ColorToken;

use super::shell::TOPBAR_HEIGHT;
use super::snapshot::UiSnapshot;
use super::UiActions;
use crate::state::{InboxItem, InboxKind};

/// The panel's width: a list of notifications hanging under its button, not a
/// document.
pub const INBOX_PANEL_WIDTH: f32 = 440.0;

/// The most of the window's height the panel takes before its list scrolls
/// inside it.
const MAX_HEIGHT_FRACTION: f32 = 0.6;

/// The room a floating panel keeps from the window's edges, and the gap under
/// the control it hangs from — the same margins the app's menus keep.
const EDGE_MARGIN: f32 = 8.0;
const PANEL_GAP: f32 = 6.0;
/// The panel's corner radius, matching the menus' own.
const PANEL_RADIUS: f32 = 8.0;

/// The side of a row's circular agent badge.
const BADGE_SIZE: f32 = 28.0;
/// The glyph inside that badge.
const BADGE_GLYPH: f32 = 14.0;

/// The banner's width: wide enough for a two-word title and its source.
const BANNER_WIDTH: f32 = 380.0;

/// Records the rect its child painted at.
///
/// The inbox panel is a window-level popover, so it cannot be the inbox
/// control's own child: the topbar would clip it and the panes below would
/// paint over it. The control reports its rect while it paints instead, and
/// [`build_inbox_popover`] hangs the panel from it one frame later — invisible,
/// because the button never moves to open it.
pub struct AnchorProbe {
    child: Box<dyn Element>,
    rect: Rc<RefCell<Option<RectF>>>,
}

impl AnchorProbe {
    pub fn new(child: Box<dyn Element>, rect: Rc<RefCell<Option<RectF>>>) -> Self {
        Self { child, rect }
    }
}

impl Element for AnchorProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let size = self.child.size().unwrap_or(Vector2F::zero());
        *self.rect.borrow_mut() = Some(RectF::new(
            PointF::new(origin.x, origin.y),
            Size2F::new(size.x, size.y),
        ));
        self.child.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }

    fn origin(&self) -> Option<Point> {
        self.child.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}

/// The panel, as the window's top layer — or nothing while the inbox is shut.
///
/// It is placed in the overlay stack rather than among the shell's children,
/// so it hangs over the bar, the sidebar and the panes without any of them
/// clipping it or taking the press aimed at it.
pub fn build_inbox_popover(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Option<Box<dyn Element>> {
    if !state.inbox_open {
        return None;
    }
    // Measured, never drawn: the same surface with its rows at their natural
    // height, which is how the popover knows the size to give the panel before
    // it caps that at a share of the window.
    let measured = panel_column(app, state, actions, false);
    let content = KeyHandler::new(
        panel_column(app, state, actions, true),
        key_handler(actions),
    )
    .finish();
    let content = Container::new(content)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(Border::all(1.0).with_border_fill(app.theme.color(ColorToken::Border)))
        .with_corner_radius(PANEL_RADIUS)
        .finish();
    let on_close = actions.on_close_inbox.clone();
    Some(
        InboxPopover::new(
            content,
            measured,
            Rc::clone(&state.inbox_anchor),
            move || (on_close.borrow_mut())(),
        )
        .finish(),
    )
}

/// The keys the open panel owns. A chorded key belongs to whatever the panel is
/// drawn over; a bare printable key belongs to the panel, so it cannot leak into
/// the composer underneath.
fn key_handler(actions: &UiActions) -> impl FnMut(&str, ModifiersState) -> bool + 'static {
    let on_escape = actions.on_close_inbox.clone();
    let on_move = actions.on_inbox_move.clone();
    let on_ack = actions.on_inbox_ack.clone();
    let on_ack_all = actions.on_inbox_ack_all.clone();
    let on_jump = actions.on_inbox_jump.clone();
    move |key: &str, modifiers| {
        if modifiers.ctrl || modifiers.command || modifiers.alt {
            return false;
        }
        match key {
            "Escape" => {
                (on_escape.borrow_mut())();
                true
            }
            "j" | "ArrowDown" => {
                (on_move.borrow_mut())(1);
                true
            }
            "k" | "ArrowUp" => {
                (on_move.borrow_mut())(-1);
                true
            }
            "a" => {
                (on_ack.borrow_mut())();
                true
            }
            "A" => {
                (on_ack_all.borrow_mut())();
                true
            }
            "Enter" => {
                (on_jump.borrow_mut())();
                true
            }
            _ => key
                .chars()
                .next()
                .map(|ch| !ch.is_control() && key.chars().count() == 1)
                .unwrap_or(false),
        }
    }
}

/// The panel's contents: header, filter chip, the rows and the key hints.
///
/// `scroll_list` gives the rows the list's own viewport, which needs the panel's
/// height to be known. The measuring copy leaves it out: its rows are laid out
/// at their natural height with no scroll state to disturb.
fn panel_column(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    scroll_list: bool,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_main_axis_size(if scroll_list {
            MainAxisSize::Max
        } else {
            MainAxisSize::Min
        })
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header(app, actions))
        .with_child(Divider::horizontal().finish())
        .with_child(chip_row(app, state));
    if let Some(error) = &state.inbox_error {
        column = column.with_child(error_line(app, error));
    }
    column = column.with_child(list(app, state, actions, scroll_list));
    column
        .with_child(Divider::horizontal().finish())
        .with_child(footer(app))
        .finish()
}

/// `Notifications` on the left, the ✕ that puts the panel away on the right.
fn header(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_close = actions.on_close_inbox.clone();
    let close = TopbarButton::new(
        Icon::new("close")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(24.0)
    .with_corner_radius(6.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_child(
                Text::new("Notifications")
                    .with_font_size(15.0)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(close)
            .finish(),
    )
    .with_padding(EdgeInsets::new(12.0, 10.0, 8.0, 10.0))
    .finish()
}

/// The filter chip: the count is the unread total, because acknowledged rows
/// keep their place in the list but no longer want the user's eyes.
fn chip_row(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    let chip = Container::new(
        Text::new(format!("All tabs ({})", state.inbox_unread_count))
            .with_font_size(12.0)
            .with_theme_color(ColorToken::Text, app)
            .with_max_lines(1)
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 3.0, 8.0, 3.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(6.0)
    .finish();
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(chip)
            .with_child(Spacer::new().finish())
            .finish(),
    )
    .with_padding(EdgeInsets::new(12.0, 8.0, 12.0, 8.0))
    .finish()
}

fn error_line(app: &AppContext, error: &str) -> Box<dyn Element> {
    Container::new(
        Text::new(error.to_string())
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Error, app)
            .with_max_lines(2)
            .finish(),
    )
    .with_padding(EdgeInsets::new(12.0, 4.0, 12.0, 4.0))
    .finish()
}

fn footer(app: &AppContext) -> Box<dyn Element> {
    Container::new(
        Text::new("j/k move · Enter jump · a ack · A ack all · Esc close")
            .with_font_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .with_max_lines(1)
            .finish(),
    )
    .with_padding(EdgeInsets::new(12.0, 6.0, 12.0, 8.0))
    .finish()
}

/// The rows, or the honest word that there are none.
fn list(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    scroll_list: bool,
) -> Box<dyn Element> {
    let body: Box<dyn Element> = if state.inbox_items.is_empty() {
        Container::new(
            Text::new("Your inbox is empty. Approvals, questions and turn reports appear here.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(2)
                .finish(),
        )
        .with_padding(EdgeInsets::new(12.0, 14.0, 12.0, 14.0))
        .finish()
    } else {
        // The rows size to their content: inside the scroll viewport the
        // available height is unbounded, so `Max` would report an infinite
        // content extent and there would be no scroll range to clamp against.
        let mut rows = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        let now = Utc::now();
        let on_jump = actions.on_inbox_jump_to.clone();
        for (index, item) in state.inbox_items.iter().enumerate() {
            if index > 0 {
                rows = rows.with_child(Divider::horizontal().finish());
            }
            rows = rows.with_child(item_row(
                app,
                state,
                item,
                index,
                index == state.inbox_index,
                Rc::clone(&on_jump),
                now,
            ));
        }
        rows.finish()
    };
    if scroll_list {
        // `Expanded` gives the list the height left after the chrome, so it
        // scrolls inside the panel instead of pushing the footer below it.
        Expanded::new(
            Scrollable::new(body, Axis::Vertical)
                .with_state(Rc::clone(&state.inbox_scroll))
                .finish(),
        )
        .finish()
    } else {
        body
    }
}

/// One notification: the agent's badge, where it happened and when, what it
/// says, and a muted line of detail. A press on it takes the user there.
fn item_row(
    app: &AppContext,
    state: &UiSnapshot,
    item: &InboxItem,
    index: usize,
    selected: bool,
    on_jump: Rc<RefCell<dyn FnMut(usize)>>,
    now: DateTime<Utc>,
) -> Box<dyn Element> {
    let title_color = if item.acknowledged {
        ColorToken::Muted
    } else {
        ColorToken::Text
    };
    let meta = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Icon::new("git-branch")
                .with_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(source_label(state, item))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Text::new(relative_time(item.created_at, now))
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    let lines = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0)
        .with_child(meta)
        .with_child(
            Text::new(item.title.clone())
                .with_font_size(13.0)
                .with_weight(FontWeight::Medium)
                .with_theme_color(title_color, app)
                .with_max_lines(2)
                .finish(),
        )
        .with_child(
            Text::new(subtitle(item))
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(10.0)
        .with_child(badge(app, item.kind))
        .with_child(Expanded::new(lines).finish())
        .finish();
    HoverRow::new(row)
        .with_selected(selected)
        .with_padding(EdgeInsets::new(12.0, 10.0, 12.0, 10.0))
        .with_on_click(move || (on_jump.borrow_mut())(index))
        .finish()
}

/// The circular badge a notification leads with: a dark disc and the kind's own
/// glyph in the kind's colour.
fn badge(app: &AppContext, kind: InboxKind) -> Box<dyn Element> {
    Container::new(
        Icon::new(kind_icon(kind))
            .with_size(BADGE_GLYPH)
            .with_theme_color(kind_color(kind), app)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform((BADGE_SIZE - BADGE_GLYPH) / 2.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .with_corner_radius(BADGE_SIZE / 2.0)
    .finish()
}

fn kind_icon(kind: InboxKind) -> &'static str {
    match kind {
        InboxKind::Approval => "terminal",
        InboxKind::Question => "info",
        InboxKind::Done | InboxKind::Task => "check",
        InboxKind::Error => "x-circle",
        InboxKind::Agent => "robot",
    }
}

fn kind_color(kind: InboxKind) -> ColorToken {
    match kind {
        InboxKind::Approval => ColorToken::Warning,
        InboxKind::Question => ColorToken::Accent,
        InboxKind::Done | InboxKind::Task => ColorToken::Success,
        InboxKind::Error => ColorToken::Error,
        InboxKind::Agent => ColorToken::Muted,
    }
}

/// The muted line under a title: what the report says in a word.
fn subtitle(item: &InboxItem) -> String {
    match item.kind {
        InboxKind::Done | InboxKind::Task => "Task completed.".to_string(),
        InboxKind::Error => "Turn failed.".to_string(),
        InboxKind::Agent => {
            if item.detail.trim().is_empty() {
                "Sub-agent failed.".to_string()
            } else {
                item.detail.clone()
            }
        }
        InboxKind::Approval | InboxKind::Question => {
            if item.detail.trim().is_empty() {
                item.kind.label().to_lowercase() + " waiting"
            } else {
                item.detail.clone()
            }
        }
    }
}

/// Where the report happened: the pane's branch when one is known, otherwise
/// the space it lives in, otherwise the conversation it belongs to.
fn source_label(state: &UiSnapshot, item: &InboxItem) -> String {
    if let Some(context) = state.composer_context.get(&item.pane_id) {
        let branch = context.branch_label.trim();
        if !branch.is_empty() {
            return branch.to_string();
        }
    }
    if let Some(space) = state.spaces.get(item.space_id) {
        if !space.name.trim().is_empty() {
            return space.name.clone();
        }
    }
    if let Some(title) = state.pane_titles.get(&item.pane_id) {
        if !title.trim().is_empty() {
            return title.clone();
        }
    }
    "Workspace".to_string()
}

/// How long ago `created_at` was, in the words the panel and the banner use.
///
/// Days are counted in whole 24-hour blocks: the first one is "yesterday", and
/// past a week the count moves to weeks so a month-old report does not read as
/// "37 days ago".
fn relative_time(created_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - created_at).num_seconds();
    if seconds < 60 {
        return "just now".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} minute{} ago", plural(minutes));
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours} hour{} ago", plural(hours));
    }
    let days = hours / 24;
    match days {
        1 => "yesterday".to_string(),
        days if days < 7 => format!("{days} days ago"),
        7..=13 => "last week".to_string(),
        weeks => format!("{} weeks ago", weeks / 7),
    }
}

fn plural(count: i64) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// The finished-turn announcement, as the window's top layer — or nothing when
/// no report is being announced.
///
/// It is drawn above the shell rather than in it, so it reads the same over the
/// workspace, over settings and over every other page, and it never shifts the
/// layout it floats above. Its time is up after
/// [`crate::state::ANNOUNCEMENT_LIFETIME`]; the frame clock runs it out.
pub fn build_announcement(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Option<Box<dyn Element>> {
    let item = state.announcement.as_ref()?;
    let on_dismiss = actions.on_announcement_dismiss.clone();
    let close = TopbarButton::new(
        Icon::new("close")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(22.0)
    .with_corner_radius(6.0)
    .with_on_click(move || (on_dismiss.borrow_mut())())
    .finish();
    let lines = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(item.title.clone())
                .with_font_size(12.5)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .with_child(
                    Icon::new("git-branch")
                        .with_size(10.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new(source_label(state, item))
                        .with_font_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(
                    Text::new(format!("· {}", subtitle(item)))
                        .with_font_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_max_lines(1)
                        .finish(),
                )
                .finish(),
        )
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(badge(app, item.kind))
        .with_child(Expanded::new(lines).finish())
        .with_child(close)
        .finish();
    let on_jump = actions.on_announcement_jump.clone();
    let content = HoverRow::new(row)
        .with_corner_radius(PANEL_RADIUS)
        .with_padding(EdgeInsets::new(12.0, 10.0, 10.0, 10.0))
        .with_on_click(move || (on_jump.borrow_mut())())
        .finish();
    let content = Container::new(
        ConstrainedBox::new(content)
            .with_width(BANNER_WIDTH)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(Border::all(1.0).with_border_fill(app.theme.color(ColorToken::Border)))
    .with_corner_radius(PANEL_RADIUS)
    .finish();
    Some(TopBanner::new(content).finish())
}

/// The anchored inbox panel.
///
/// It takes no room in the stack it is mounted in: it is painted at its anchor
/// and hit-tests against the rect it painted at, so placement and input agree by
/// construction (the same seam as [`goble_ui::elements::PointerMenu`], with the
/// anchor coming from the control that opened it rather than from the pointer).
struct InboxPopover {
    /// What is drawn and what the keys and clicks reach.
    content: Box<dyn Element>,
    /// The same surface with the list unclipped, laid out only to measure the
    /// height the panel would like to have.
    measured: Box<dyn Element>,
    anchor: Rc<RefCell<Option<RectF>>>,
    on_close: Rc<RefCell<dyn FnMut()>>,
    window: Option<Vector2F>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl InboxPopover {
    fn new(
        content: Box<dyn Element>,
        measured: Box<dyn Element>,
        anchor: Rc<RefCell<Option<RectF>>>,
        on_close: impl FnMut() + 'static,
    ) -> Self {
        Self {
            content,
            measured,
            anchor,
            on_close: Rc::new(RefCell::new(on_close)),
            window: None,
            size: None,
            origin: None,
        }
    }

    /// The width the panel is laid out at: its own, less the edges of a window
    /// too narrow to hold it.
    fn width(&self) -> f32 {
        match self.window {
            Some(window) => INBOX_PANEL_WIDTH.min((window.x - EDGE_MARGIN * 2.0).max(160.0)),
            None => INBOX_PANEL_WIDTH,
        }
    }

    fn cap(&self) -> Option<f32> {
        self.window
            .map(|window| (window.y * MAX_HEIGHT_FRACTION).max(120.0))
    }

    /// Where the panel hangs: trailing-aligned under the inbox control, a gap
    /// below it, and never past a window edge.
    fn place(&self, size: Vector2F) -> Vector2F {
        let anchor = *self.anchor.borrow();
        let Some(window) = self.window else {
            // An unbounded frame (a tree laid out loose, as the offline tests
            // lay one out): there is no edge to clamp against.
            return match anchor {
                Some(anchor) => vec2f(anchor.max_x() - size.x, anchor.max_y() + PANEL_GAP),
                None => vec2f(0.0, TOPBAR_HEIGHT + PANEL_GAP),
            };
        };
        let (x, y) = match anchor {
            Some(anchor) => (anchor.max_x() - size.x, anchor.max_y() + PANEL_GAP),
            // The control has not painted yet: open under the bar's own right
            // edge, which is where the inbox lives.
            None => (window.x - size.x - EDGE_MARGIN, TOPBAR_HEIGHT + PANEL_GAP),
        };
        vec2f(
            x.min(window.x - size.x - EDGE_MARGIN).max(EDGE_MARGIN),
            y.min(window.y - size.y - EDGE_MARGIN).max(EDGE_MARGIN),
        )
    }
}

impl Element for InboxPopover {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.window = constraint.max.x.is_finite().then_some(constraint.max);
        let width = self.width();
        let wanted =
            self.measured
                .layout(SizeConstraint::loose(vec2f(width, f32::INFINITY)), ctx, app);
        let height = match self.cap() {
            Some(cap) => wanted.y.min(cap),
            None => wanted.y,
        };
        let size = self
            .content
            .layout(SizeConstraint::loose(vec2f(width, height)), ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let Some(size) = self.size else {
            return;
        };
        let at = self.place(size);
        self.origin = Some(Point::from_vec2f(at, Default::default()));
        self.content.paint(at, ctx, app);
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
        // Keys and the wheel carry no position to hit-test with; both belong to
        // the panel while it is up.
        if matches!(
            event,
            DispatchedEvent::KeyDown { .. } | DispatchedEvent::Scroll { .. }
        ) {
            return self.content.dispatch_event(event, ctx, app);
        }
        let Some(panel) = self.bounds() else {
            return false;
        };
        let Some(position) = event_position(event) else {
            return false;
        };
        if panel.contains(PointF::new(position.x, position.y)) {
            let consumed = self.content.dispatch_event(event, ctx, app);
            // The panel is a surface, not only its rows: a press in the gap
            // between two controls is still a press on the open panel.
            return consumed
                || matches!(
                    event,
                    DispatchedEvent::MouseDown { .. } | DispatchedEvent::MouseUp { .. }
                );
        }
        // A press on the control that opened the panel is the control's own:
        // its click toggles the panel shut rather than being swallowed here.
        if self
            .anchor
            .borrow()
            .is_some_and(|anchor| anchor.contains(PointF::new(position.x, position.y)))
        {
            return false;
        }
        // Anywhere else is a dismissal. The press is not consumed: it still
        // lands on whatever the user aimed at, because nothing is dimmed and
        // nothing is trapped.
        if matches!(
            event,
            DispatchedEvent::MouseDown {
                button: BUTTON_PRIMARY,
                ..
            }
        ) {
            (self.on_close.borrow_mut())();
        }
        false
    }
}

/// The pointer position carried by a mouse event, if any.
fn event_position(event: &DispatchedEvent) -> Option<Vector2F> {
    match event {
        DispatchedEvent::MouseDown { position, .. }
        | DispatchedEvent::MouseUp { position, .. }
        | DispatchedEvent::MouseMove { position } => Some(*position),
        _ => None,
    }
}

/// The announcement's own placement: centered at the top of the window, just
/// under the bar. It takes no room in the tree it is mounted in, and forwards
/// every event to its content, whose controls hit-test themselves.
struct TopBanner {
    content: Box<dyn Element>,
    window: Option<Vector2F>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TopBanner {
    fn new(content: Box<dyn Element>) -> Self {
        Self {
            content,
            window: None,
            size: None,
            origin: None,
        }
    }

    fn place(&self, size: Vector2F) -> Vector2F {
        let Some(window) = self.window else {
            return vec2f(0.0, TOPBAR_HEIGHT + EDGE_MARGIN);
        };
        let x = ((window.x - size.x) / 2.0)
            .min(window.x - size.x - EDGE_MARGIN)
            .max(EDGE_MARGIN);
        vec2f(x, TOPBAR_HEIGHT + EDGE_MARGIN)
    }
}

impl Element for TopBanner {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.window = constraint.max.x.is_finite().then_some(constraint.max);
        let size = self.content.layout(
            SizeConstraint::loose(vec2f(BANNER_WIDTH, f32::INFINITY)),
            ctx,
            app,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let Some(size) = self.size else {
            return;
        };
        let at = self.place(size);
        self.origin = Some(Point::from_vec2f(at, Default::default()));
        self.content.paint(at, ctx, app);
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
        self.content.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod ui_tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{
        AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
    };
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{vec2f, RectF, Vector2F};
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::theme::ColorToken;

    use super::relative_time;
    use super::INBOX_PANEL_WIDTH;
    use crate::root_view::RootView;
    use crate::state::{InboxKind, UiState};
    use crate::ui::shell::TOPBAR_HEIGHT;

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    fn root() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
        }
        (Box::new(view), state, dir)
    }

    fn drawn_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The rect the panel paints its surface at.
    fn panel_rect(app: &AppContext, commands: &[RenderCommand]) -> RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Surface)
                        && (rect.width() - INBOX_PANEL_WIDTH).abs() < 0.5 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the panel background is drawn")
    }

    fn press(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        key: &str,
        modifiers: ModifiersState,
    ) -> bool {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let mut paint = PaintContext::default();
        root.paint(vec2f(0.0, 0.0), &mut paint, app);
        let mut ctx = EventContext;
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers,
            },
            &mut ctx,
            app,
        )
    }

    /// Lay the tree out and paint a frame, then send a click at `at`.
    fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) -> bool {
        let _ = root.layout(SizeConstraint::loose(window()), &mut LayoutContext, app);
        let mut paint = PaintContext::default();
        root.paint(vec2f(0.0, 0.0), &mut paint, app);
        let mut ctx = EventContext;
        let down = root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            &mut ctx,
            app,
        );
        let up = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
            &mut ctx,
            app,
        );
        down || up
    }

    /// The centre of the topbar's inbox glyph.
    fn inbox_button_center(commands: &[RenderCommand]) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, size, name, ..
                } if name == "inbox-01" && origin.y < TOPBAR_HEIGHT => {
                    Some(*origin + vec2f(size / 2.0, size / 2.0))
                }
                _ => None,
            })
            .expect("the topbar draws the inbox control")
    }

    #[test]
    fn the_popover_lists_items_newest_first_with_the_live_unread_count() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.raise_inbox(
                1,
                "c1",
                InboxKind::Approval,
                "Approve the command",
                "git status",
            );
            s.raise_inbox(
                1,
                "c1",
                InboxKind::Question,
                "Which branch?",
                "main or release",
            );
            s.inbox_open = true;
        }
        let commands = render_element(&mut root, window(), &app);
        let texts = drawn_texts(&commands);
        assert!(texts.iter().any(|text| text == "Notifications"));
        assert!(
            texts.iter().any(|text| text == "All tabs (2)"),
            "the chip carries the unread count: {texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Which branch?"));
        assert!(texts.iter().any(|text| text == "Approve the command"));
        assert!(
            texts.iter().any(|text| text == "just now"),
            "a row says how long ago it happened: {texts:?}"
        );
        assert_eq!(
            state.borrow().inbox[0].kind,
            InboxKind::Question,
            "the newest item is drawn first"
        );
        assert!(
            topbar_texts(&commands).iter().any(|text| text == "2"),
            "the topbar control shows the same unread count: {:?}",
            topbar_texts(&commands)
        );
    }

    /// The chip counts what still wants the user's eyes: acknowledged rows keep
    /// their place in the list but leave the count.
    #[test]
    fn the_filter_chip_counts_the_unread_rows() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            let first = s.raise_inbox(1, "c1", InboxKind::Approval, "Approve", "git status");
            s.raise_inbox(1, "c2", InboxKind::Question, "Which branch?", "main");
            s.raise_inbox(1, "c3", InboxKind::Done, "Turn finished", "done");
            s.ack_inbox(&first);
            s.inbox_open = true;
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "All tabs (2)"),
            "three rows are drawn, one of them acknowledged: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "Approve"),
            "the acknowledged row is still listed"
        );
    }

    /// The control toggles the panel: the press it takes is its own, not the
    /// popover's outside-close.
    #[test]
    fn the_inbox_button_toggles_the_panel_open_and_shut() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let commands = render_element(&mut root, window(), &app);
        let button = inbox_button_center(&commands);

        click(&mut root, &app, button);
        assert!(state.borrow().inbox_open, "the button opens the panel");
        assert!(
            state.borrow().inbox_anchor.borrow().is_some(),
            "the control recorded its rect for the panel to hang from"
        );

        click(&mut root, &app, button);
        assert!(!state.borrow().inbox_open, "the same button shuts it");
    }

    #[test]
    fn a_populated_panel_keeps_the_footer_and_list_inside_the_viewport() {
        let app = AppContext::default();
        for (width, height) in [(768.0_f32, 600.0_f32), (600.0, 400.0)] {
            let (mut root, state, _dir) = root();
            {
                let mut s = state.borrow_mut();
                // Enough rows that the list has to scroll in either window.
                for index in 0..30 {
                    s.raise_inbox(
                        1,
                        format!("c{index}"),
                        InboxKind::Approval,
                        format!("Approval {index}"),
                        format!("command {index}"),
                    );
                }
                s.inbox_open = true;
            }
            let _ = render_element(&mut root, vec2f(width, height), &app);
            let commands = render_element(&mut root, vec2f(width, height), &app);
            let (footer_y, footer_height) = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText {
                        text,
                        origin,
                        line_height,
                        ..
                    } if text.starts_with("j/k move") => Some((origin.y, *line_height)),
                    _ => None,
                })
                .expect("the footer is drawn");
            assert!(
                footer_y + footer_height <= height,
                "the footer stays in a {width}x{height} window: its bottom is {}",
                footer_y + footer_height
            );

            let panel = panel_rect(&app, &commands);
            assert!(
                panel.max_y() <= height,
                "the panel is constrained to the viewport: {} > {height}",
                panel.max_y()
            );
            assert!(
                footer_y >= panel.min_y() && footer_y + footer_height <= panel.max_y(),
                "the footer sits inside the panel, not below it"
            );

            let viewport = state.borrow().inbox_scroll.borrow().viewport();
            assert!(viewport > 0.0, "the list has a viewport");
            assert!(
                viewport <= panel.height() - 60.0,
                "the list viewport stops above the footer instead of using the \
                 whole panel height: viewport {viewport}, panel {}",
                panel.height()
            );
        }
    }

    #[test]
    fn the_overlay_shows_an_honest_empty_state() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        state.borrow_mut().inbox_open = true;
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Your inbox is empty")),
            "an empty inbox says so: {texts:?}"
        );
    }

    #[test]
    fn the_overlay_shows_the_reason_a_jump_did_not_land() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.inbox_error =
                Some("Pane 9 is no longer open; the item has nothing to jump to.".to_string());
            s.inbox_open = true;
        }
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Pane 9 is no longer open")),
            "the error is drawn, not swallowed: {texts:?}"
        );
    }

    #[test]
    fn the_overlays_keys_move_ack_jump_and_close() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.raise_inbox(
                1,
                "c1",
                InboxKind::Approval,
                "Approve the command",
                "git status",
            );
            s.raise_inbox(
                1,
                "c1",
                InboxKind::Question,
                "Which branch?",
                "main or release",
            );
            s.inbox_open = true;
        }
        assert!(press(&mut root, &app, "j", ModifiersState::none()));
        assert_eq!(state.borrow().inbox_index, 1, "j walks down");
        assert!(press(&mut root, &app, "k", ModifiersState::none()));
        assert_eq!(state.borrow().inbox_index, 0, "k walks up");
        assert!(press(&mut root, &app, "a", ModifiersState::none()));
        assert_eq!(
            state.borrow().unread_inbox_count(),
            1,
            "a acks the selected row"
        );
        assert!(press(&mut root, &app, "A", ModifiersState::none()));
        assert_eq!(state.borrow().unread_inbox_count(), 0, "A acks all");
        assert!(press(&mut root, &app, "Enter", ModifiersState::none()));
        assert!(
            !state.borrow().inbox_open,
            "Enter jumps and puts the panel away"
        );
        // Reopen and close with Escape.
        state.borrow_mut().inbox_open = true;
        assert!(press(&mut root, &app, "Escape", ModifiersState::none()));
        assert!(!state.borrow().inbox_open);
    }

    #[test]
    fn cmd_shift_i_toggles_the_inbox() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        let chord = ModifiersState {
            command: true,
            shift: true,
            ..Default::default()
        };
        assert!(press(&mut root, &app, "i", chord));
        assert!(state.borrow().inbox_open, "Cmd+Shift+I opens the inbox");
        assert!(press(&mut root, &app, "i", chord));
        assert!(!state.borrow().inbox_open, "the chord toggles it shut");
    }

    /// The panel is not a dialog: it hangs from the inbox control, trailing
    /// aligned with it and below it, and stays inside the window.
    #[test]
    fn the_panel_hangs_under_the_inbox_button_inside_the_window() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.raise_inbox(
                1,
                "c1",
                InboxKind::Approval,
                "Approve the command",
                "git status",
            );
            s.inbox_open = true;
        }
        // The control records its rect while it paints, so the frame after it
        // is the one that knows where the panel hangs.
        let _ = render_element(&mut root, window(), &app);
        let commands = render_element(&mut root, window(), &app);
        let panel = panel_rect(&app, &commands);
        let anchor = state
            .borrow()
            .inbox_anchor
            .borrow()
            .expect("the inbox control painted its rect");

        assert!(
            (panel.max_x() - anchor.max_x()).abs() < 0.5,
            "the panel's trailing edge sits on the control's: {panel:?} against {anchor:?}"
        );
        assert!(
            panel.min_y() >= anchor.max_y(),
            "the panel hangs below the control, not over it: {panel:?} against {anchor:?}"
        );
        assert!(
            panel.max_y() <= window().y && panel.max_x() <= window().x,
            "the panel stays inside the {:?} window: {panel:?}",
            window()
        );
        assert!(
            panel.max_y() > TOPBAR_HEIGHT,
            "the panel opens below the bar: {panel:?}"
        );
        // It is the popover's rect, not a centered dialog's: the panel is
        // nowhere near the middle of the window's width.
        let center = panel.min_x() + panel.width() / 2.0;
        assert!(
            center > window().x / 2.0,
            "the panel is anchored at the trailing end: {panel:?}"
        );
    }

    /// Nothing is dimmed and nothing is trapped: a press outside the panel
    /// closes it, and the click still reaches the control it was aimed at.
    #[test]
    fn a_press_outside_the_panel_closes_it_and_lands_underneath() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        state.borrow_mut().inbox_open = true;
        let commands = render_element(&mut root, window(), &app);
        let panel = panel_rect(&app, &commands);
        let outside = vec2f(panel.min_x() - 40.0, panel.max_y() + 40.0);
        click(&mut root, &app, outside);
        assert!(
            !state.borrow().inbox_open,
            "a press outside the panel puts it away"
        );

        // The tools toggle is drawn in the bar the panel hangs under: with the
        // panel open, a press on it is the toggle's, not a backdrop's.
        state.borrow_mut().inbox_open = true;
        let commands = render_element(&mut root, window(), &app);
        let toggle = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, size, name, ..
                } if name == "menu-01" && origin.y < TOPBAR_HEIGHT => {
                    Some(*origin + vec2f(size / 2.0, size / 2.0))
                }
                _ => None,
            })
            .expect("the topbar draws the tools toggle");
        let was_visible = state.borrow().sidebar_visible;
        click(&mut root, &app, toggle);
        assert_ne!(
            state.borrow().sidebar_visible,
            was_visible,
            "the control under the open panel still takes its click"
        );
        assert!(
            !state.borrow().inbox_open,
            "and the panel steps out of the way"
        );
    }

    /// A finished turn is announced at the top of the window, wherever the user
    /// is looking.
    #[test]
    fn a_done_item_is_announced_with_its_title_and_source() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        state.borrow_mut().raise_inbox(
            1,
            "c1",
            InboxKind::Done,
            "Turn finished",
            "2 files changed",
        );
        assert!(
            state.borrow().announcement.is_some(),
            "the report is announced"
        );
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            texts.iter().any(|text| text == "Turn finished"),
            "the banner carries the title: {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text.contains("Task completed.")),
            "and the word that it finished: {texts:?}"
        );
        // The banner names the branch the work happened on, which is whatever
        // the pane's composer reports rather than a branch spelled here.
        let branch = state.borrow().composer_branch.clone();
        assert!(
            texts.iter().any(|text| *text == branch),
            "and the branch it happened on ({branch:?}): {texts:?}"
        );
    }

    /// The banner goes away when it is dismissed, and the item stays.
    #[test]
    fn dismissing_the_announcement_removes_the_banner() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.raise_inbox(
                1,
                "c1",
                InboxKind::Error,
                "Turn failed",
                "the model refused",
            );
            s.dismiss_announcement();
        }
        assert!(state.borrow().announcement.is_none());
        assert_eq!(
            state.borrow().inbox.len(),
            1,
            "the report stays in the list"
        );
        let texts = drawn_texts(&render_element(&mut root, window(), &app));
        assert!(
            !texts.iter().any(|text| text.contains("Turn failed.")),
            "a dismissed banner is not drawn: {texts:?}"
        );
    }

    /// The announcement is not a workspace feature: it is drawn over whatever
    /// page is up.
    #[test]
    fn the_announcement_is_drawn_over_the_settings_page() {
        let app = AppContext::default();
        let (mut root, state, _dir) = root();
        {
            let mut s = state.borrow_mut();
            s.open_settings_tab(None);
            s.raise_inbox(1, "c1", InboxKind::Done, "Turn finished", "2 files changed");
        }
        let commands = render_element(&mut root, window(), &app);
        assert!(
            drawn_texts(&commands)
                .iter()
                .any(|text| text == "Turn finished"),
            "the banner is drawn over settings: {:?}",
            drawn_texts(&commands)
        );
        // The banner's surface is the one the title is drawn on, not whichever
        // raised fill the page under it happens to paint first.
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "Turn finished" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the banner's title is drawn");
        let panel = commands
            .iter()
            .rev()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::SurfaceRaised)
                        && rect.min_x() <= title.x
                        && rect.max_x() >= title.x
                        && rect.min_y() <= title.y
                        && rect.max_y() >= title.y =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the banner has its own surface");
        assert!(
            panel.max_y() > TOPBAR_HEIGHT,
            "the banner sits below the bar: {panel:?}"
        );
        assert!(
            panel.max_y() < window().y / 2.0,
            "the banner sits at the top of the window: {panel:?}"
        );
    }

    /// Just now, minutes, hours, yesterday and the days after.
    #[test]
    fn the_relative_time_reads_the_way_a_notification_does() {
        let now = Utc::now();
        let ago = |seconds: i64| relative_time(now - Duration::seconds(seconds), now);
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(5 * 60), "5 minutes ago");
        assert_eq!(ago(59 * 60), "59 minutes ago");
        assert_eq!(ago(60 * 60), "1 hour ago");
        assert_eq!(ago(2 * 60 * 60 + 30), "2 hours ago");
        assert_eq!(ago(23 * 60 * 60), "23 hours ago");
        assert_eq!(ago(24 * 60 * 60), "yesterday");
        assert_eq!(ago(2 * 24 * 60 * 60), "2 days ago");
        assert_eq!(ago(8 * 24 * 60 * 60), "last week");
        assert_eq!(ago(21 * 24 * 60 * 60), "3 weeks ago");
        // A clock that is a second ahead of the report still reads as now.
        assert_eq!(relative_time(now + Duration::seconds(5), now), "just now");
    }
}
