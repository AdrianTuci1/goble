use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, AppMark, Border, Container, CrossAxisAlignment, EdgeInsets, Element, Empty,
    EventContext, Expanded, Fill, Flex, HoverRow, Icon, LayoutContext, MainAxisAlignment,
    MainAxisSize, MenuClose, MenuId, MenuPointer, Padding, PaintContext, Point, SizeConstraint,
    Stack, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::platform::text_atlas::{wrapped_lines, FontWeight};
use crate::theme::{ColorToken, FontFamily, SpacingToken};

/// Extra height above and below a card's rows. The card is a full-width band,
/// so without it the card's rows sit flush against the cards above and below
/// and the digest reads as one block of text.
const CARD_EXTRA_HEIGHT: f32 = 4.0;

/// The 3-dot control's box, which is the glyph's own box: what the control
/// occupies and what the user sees are the same 16 points, so an inset measured
/// from the control is the gap the dots have to the surface's edge.
const DOTS_SIZE: f32 = 16.0;

/// Room the row's content keeps clear to the left of the control. A card's text
/// is measured with no width to wrap against (it is one line, cut at the card's
/// edge), so the clip is what stops the title short of the dots.
const DOTS_CLEARANCE: f32 = 6.0;

/// The inset the card's content keeps from the band's edges. The band itself
/// stays full width — its hover and selected fill spans the sidebar, and the
/// card's hit area with it — so this is what keeps the content off the
/// surface's edge, the 12 pt a warp-new list item keeps for its own content
/// (`Container::with_horizontal_padding(12.)`).
const CARD_CONTENT_INSET: f32 = 12.0;

/// The harness mark the card leads with. It takes the space the status square
/// had, and the status becomes the badge on its corner.
const HARNESS_SIZE: f32 = 16.0;

/// The gap between the harness mark and the subject.
const HARNESS_GAP: f32 = 6.0;

/// The status badge's diameter. Small enough to read as a dot on the mark's
/// corner rather than a second glyph.
const STATUS_BADGE_SIZE: f32 = 8.0;

/// How far the badge hangs past the mark's bottom-right corner, so it reads as
/// a badge on the corner rather than a dot inside the glyph.
const STATUS_BADGE_OVERHANG: f32 = 2.0;

/// Where the badge's top-left sits relative to the mark's: the mark's corner
/// minus the badge's size plus the overhang.
const STATUS_BADGE_OFFSET: f32 = HARNESS_SIZE - STATUS_BADGE_SIZE + STATUS_BADGE_OVERHANG;

/// The badge's ring, painted in the card's own background so the dot knocks a
/// clean hole out of the mark it sits on.
const STATUS_BADGE_RING: f32 = 1.5;

/// The gutter the harness mark keeps before the subject: the mark and the gap
/// after it. It is what the rows under the title pad themselves by to line up
/// with the subject, the content inset coming from their container.
const TEXT_GUTTER: f32 = HARNESS_SIZE + HARNESS_GAP;

/// The inset the card's text starts at: the content inset plus the gutter the
/// status square and its gap keep for themselves. The container supplies the
/// content inset and the title row the gutter, so only the tests need the sum.
#[cfg(test)]
const TEXT_INSET: f32 = CARD_CONTENT_INSET + TEXT_GUTTER;

/// The widest the card's tray grows before its rows' own width is what it takes.
/// Wide enough for "Delete conversation" on one line with the row's own
/// padding, since the tray now opens beside the dots where that width is free.
const TRAY_MAX_WIDTH: f32 = 200.0;

/// How far the tray's top sits below the card the control is on.
const TRAY_GAP: f32 = 4.0;

/// The tray's own corner radius, inset and hairline: the card is the same one
/// the rich input's controls open beside themselves, so the two trays read as
/// one surface (see `PopupMenu`). The chat card's own tray keeps this; the
/// sidebar rows use the DSH context-menu metrics below.
const TRAY_RADIUS: f32 = 6.0;
/// The tray's vertical padding, and the horizontal inset each of its rows keeps
/// for its own content. The tray itself has no horizontal padding: a row's band
/// spans the tray edge to edge.
const TRAY_PADDING: f32 = 8.0;

// -- the sidebar rows' context menus (DSH's `Menu` primitive) ----------------
//
// The sidebar's workspace and session trays are DSH context menus: the shared
// `Menu` card in `packages/client/ui-primitives/src/Menu.module.css` plus the
// `MenuSurface` radii/tokens in `MenuSurface.module.css` and
// `ui-theme/src/styles/{base,design-platform,gradient-shadow-text}.css`:
//   .list     padding 4px; min-width 144px; max-width 360px; radius-lg 16px
//   .item     min-height 34px; padding 6px 8px; gap 6px; radius-md 12px;
//             font-size 13px; line-height 20px
//   .itemIcon 14px, `--dsw-alias-menu-icon`
//   .separator height .5px; margin 3px 8px; `--dsw-alias-border-l2`
//   .danger   `--dsw-alias-state-error-primary` text/icon, danger hover fill
//   hover/focus fill `--dsw-alias-interactive-bg-hover` (the selected row's
//             `selectedFill` uses the same step: DSH has one visible hover fill)
// Goble maps those onto its own tokens: `SurfaceRaised` + `Border` casing,
// `Text` label / `Muted` icon, `Error` for danger, and `Selected` for the
// hover/selection band (the plain `Hover` token equals the raised surface's own
// colour, so it would paint no band on the card).

/// DSH's menu minimum width (`.list { min-width: 144px }`).
const SIDEBAR_MENU_MIN_WIDTH: f32 = 144.0;
/// The widest a sidebar tray grows: DSH's `.list` caps at 360px, a sidebar row's
/// menu keeps inside the sidebar's own budget.
const SIDEBAR_MENU_MAX_WIDTH: f32 = 240.0;
/// The casing's own padding (DSH's `.list { padding: 4px }`).
const SIDEBAR_MENU_PADDING: f32 = 4.0;
/// The casing's radius: DSH's `--dsw-radius-lg` (16px).
const SIDEBAR_MENU_RADIUS: f32 = 16.0;
/// A row band's radius: DSH's `--dsw-radius-md` (12px).
const SIDEBAR_MENU_ROW_RADIUS: f32 = 12.0;
/// A row's height: DSH's `.item { min-height: 34px }` (the label's 20px line
/// plus the 6px padding above and below is 32, so the row pads to 34).
const SIDEBAR_MENU_ROW_HEIGHT: f32 = 34.0;
/// A row's label: DSH's `.item { font-size: 13px; line-height: 20px }`.
const SIDEBAR_MENU_LABEL_SIZE: f32 = 13.0;
const SIDEBAR_MENU_LABEL_LINE: f32 = 20.0;
/// The gap between a row's icon and its label (DSH `.item { gap: 6px }`).
const SIDEBAR_MENU_ICON_GAP: f32 = 6.0;
/// A row's leading icon box (DSH `.itemIcon { width/height: 14px }`).
const SIDEBAR_MENU_ICON_SIZE: f32 = 14.0;
/// A row's own horizontal padding inside the casing (DSH `.item { padding: 6px
/// 8px }`; the 4px casing padding puts the content 12px inside the card).
const SIDEBAR_MENU_ROW_PADDING_X: f32 = 8.0;
/// A separator's rule and breathing room (DSH `.separator { height: .5px;
/// margin: 3px 8px }`).
const SIDEBAR_MENU_SEPARATOR_HEIGHT: f32 = 0.5;
const SIDEBAR_MENU_SEPARATOR_MARGIN: f32 = 3.0;
const SIDEBAR_MENU_SEPARATOR_INSET: f32 = 8.0;

/// The tray's rows, in paint order: the star toggle first, then delete.
const STAR_ROW: usize = 0;
const DELETE_ROW: usize = 1;

/// The sidebar trays' "no row highlighted yet" marker. DSH opens a context
/// menu with its first row focused but not filled (the menu's hover fill only
/// appears for a pointer hover or a keyboard walk), so the app opens a sidebar
/// tray with no painted highlight; Enter still runs the first row.
const NO_HIGHLIGHT: usize = usize::MAX;

/// Per-card interaction state that must survive the per-frame element rebuild.
/// Owned by the app (a map keyed by conversation id) and shared with the card
/// through `Rc<RefCell<_>>`, so hover / the delete menu persist across frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct AgentCardUi {
    pub hover: bool,
    pub menu_open: bool,
    /// The tray's highlighted row, [`STAR_ROW`] or [`DELETE_ROW`]. The app owns
    /// it with the rest of the card's state so the keyboard selection survives
    /// the rebuild; it is reset when the tray closes.
    pub highlight: usize,
    /// When the pointer reached the row, for the hover card's 800ms dwell.
    /// `None` while the pointer is away.
    pub hover_card_since: Option<std::time::Instant>,
    /// Whether the hover card has finished its dwell and is drawn.
    pub hover_card_open: bool,
    /// How far the hovered title's marquee has crawled, in points.
    pub marquee_offset: f32,
    /// The last frame the marquee advanced on, so it travels at a constant
    /// speed rather than one frame-width per rebuild.
    pub marquee_last: Option<std::time::Instant>,
}

impl AgentCardUi {
    /// Put the tray away with its highlight back on the first row, where the
    /// next open starts.
    pub fn close_menu(&mut self) {
        self.menu_open = false;
        self.highlight = STAR_ROW;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConversationStatus {
    #[default]
    Default,
    /// A turn is writing into the conversation: the sidebar row draws the
    /// running ring in its leading slot.
    Running,
    /// The agent is waiting on the user (an approval or a question): the
    /// sidebar row draws the amber dot.
    Waiting,
    Success,
    Error,
    Stopped,
}

impl ConversationStatus {
    /// The colour the card's status square carries: gray for the quiet states,
    /// yellow while the agent is waiting on the user, red for a failed turn and
    /// green for one that finished. The running ring draws muted (DSH's spinner
    /// uses the tertiary ink).
    pub fn color_token(self) -> ColorToken {
        match self {
            Self::Running => ColorToken::Muted,
            Self::Waiting => ColorToken::Warning,
            Self::Success => ColorToken::Success,
            Self::Error => ColorToken::Error,
            Self::Stopped | Self::Default => ColorToken::Muted,
        }
    }
}

/// The atlas glyph for a harness key. Provider names (`anthropic`, `openai`,
/// `xai`, `google`) are accepted beside the harness ids so a conversation that
/// only recorded its model provider still gets its family's mark, and every id
/// in the shared catalog gets its own; an unknown key draws Goble's own.
pub fn harness_icon(harness: &str) -> &'static str {
    goble_core::harness::harness_for(harness).map_or("harness-goble", |spec| spec.icon)
}

/// A harness's mark at `size`, as a surface that leads with it (a sidebar card)
/// draws it: Goble's own artwork for the native harness — the atlas glyph for it
/// is a rounded square, which one tint flattens to a block — and the catalog
/// glyph in the family's accent for every other harness. A key this build does
/// not know draws the native mark, like the sidebar card does.
pub fn harness_mark(harness: &str, size: f32, app: &AppContext) -> Box<dyn Element> {
    let spec = goble_core::harness::harness_for(harness);
    if spec.map_or(true, |spec| spec.id == "goble") {
        return AppMark::new(size).finish();
    }
    Icon::new(harness_icon(harness))
        .with_size(size)
        .with_color(harness_accent(harness, app))
        .finish()
}

/// The same mark for a bar that names the harness beside a title: the octopus
/// alone for the native harness, with none of the app's square frame around it,
/// and the catalog glyph in the family's accent for every other harness.
pub fn harness_bar_mark(harness: &str, size: f32, app: &AppContext) -> Box<dyn Element> {
    let spec = goble_core::harness::harness_for(harness);
    if spec.map_or(true, |spec| spec.id == "goble") {
        return AppMark::octopus(size).finish();
    }
    Icon::new(harness_icon(harness))
        .with_size(size)
        .with_color(harness_accent(harness, app))
        .finish()
}

/// The colour a harness's mark is drawn in: the harness family's own accent, so
/// a bar carrying the mark reads which agent it is without naming it.
///
/// The marks are single-tint glyphs, so this is the one colour each of them
/// takes. A harness with no brand colour of its own — and one this build does
/// not know — draws in the theme's muted ink.
pub fn harness_accent(harness: &str, app: &AppContext) -> crate::ColorU {
    let id = goble_core::harness::harness_for(harness).map_or("goble", |spec| spec.id);
    match id {
        "goble" => app.theme.color(ColorToken::Accent),
        "claude" => crate::ColorU::new(0xD9, 0x77, 0x57, 255),
        "codex" => crate::ColorU::new(0x74, 0xAA, 0x9C, 255),
        "gemini" => crate::ColorU::new(0x42, 0x85, 0xF4, 255),
        "grok" => app.theme.color(ColorToken::Text),
        "cursor" => crate::ColorU::new(0xB4, 0xB4, 0xB4, 255),
        "copilot" => crate::ColorU::new(0x8B, 0x5C, 0xF6, 255),
        _ => app.theme.color(ColorToken::Muted),
    }
}

pub struct ConversationListItem {
    id: String,
    name: String,
    timestamp: String,
    /// The directory the conversation works in, drawn under its subject. `None`
    /// (or empty) draws no row, so a conversation that has not run anywhere yet
    /// keeps the single-line card.
    directory: Option<String>,
    /// Where the conversation runs: `"remote"` for one on a worker, which the
    /// directory row marks with the cloud. A local conversation draws nothing
    /// extra — the machine it runs on is this one.
    workspace_routing: String,
    selected: bool,
    /// Whether the conversation is starred, which the card marks and the
    /// sidebar's Starred section lists.
    starred: bool,
    /// What the badge on the harness mark reports. [`ConversationStatus::Default`]
    /// and [`ConversationStatus::Stopped`] draw the neutral gray dot.
    status: ConversationStatus,
    /// Which harness the conversation runs on. The card leads with its mark.
    harness: String,
    ui: Rc<RefCell<AgentCardUi>>,
    on_select: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_toggle_star: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// How far the 3-dot control's trailing edge sits from the card's trailing
    /// edge, in logical points. The caller subtracts its own padding from the
    /// gap it wants from the surface's edge: the sidebar passes `5 - 4`, so the
    /// dots sit 5 pt inside the sidebar's width whatever the card's own inset
    /// from that edge is.
    dots_inset: f32,
    /// The row: the status square and the conversation's own text, painted
    /// clipped clear of the control.
    row: Option<Box<dyn Element>>,
    /// The card's tray: star/unstar and delete, in a card of its own anchored to
    /// the 3-dot control. Held as a shared handle because the frame's last paint
    /// layer draws it (so the cards painted after this one cannot cover it)
    /// while this element keeps the same handle to lay it out and to dispatch
    /// clicks into it.
    menu: Option<Rc<RefCell<Box<dyn Element>>>>,
    /// The tray's size from the last layout: what its anchored position is
    /// computed from.
    menu_size: Option<Vector2F>,
    /// The 3-dot control, drawn while the card is hovered or its menu is open.
    /// Held out of the row's flex children on purpose: a child at the end of the
    /// row is placed after whatever the title measured, and a title wider than
    /// the card carries the control off the card's trailing edge.
    dots: Option<Box<dyn Element>>,
    /// The row's height from the last layout: where the menu starts and what the
    /// control is centred against.
    row_height: f32,
    bg: crate::color::ColorU,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationListItem {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        timestamp: impl Into<String>,
        ui: Rc<RefCell<AgentCardUi>>,
        selected: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            timestamp: timestamp.into(),
            directory: None,
            workspace_routing: "local".to_string(),
            selected,
            starred: false,
            status: ConversationStatus::Default,
            harness: "goble".to_string(),
            ui,
            on_select: None,
            on_delete: None,
            on_toggle_star: None,
            dots_inset: 0.0,
            row: None,
            menu: None,
            menu_size: None,
            dots: None,
            row_height: 0.0,
            bg: crate::color::ColorU::default(),
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the conversation is starred. A starred card marks itself, so a
    /// card that also sits in the sidebar's Starred section still says why.
    pub fn with_starred(mut self, starred: bool) -> Self {
        self.starred = starred;
        self
    }

    /// Star or unstar the conversation, from the card's own menu.
    pub fn with_on_toggle_star<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_star = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set what the card's status square reports: gray, yellow, red or green,
    /// one per [`ConversationStatus`].
    pub fn with_status(mut self, status: ConversationStatus) -> Self {
        self.status = status;
        self
    }

    /// Set which harness the conversation runs on. Unknown keys draw the
    /// native Goble mark.
    pub fn with_harness(mut self, harness: impl Into<String>) -> Self {
        self.harness = harness.into();
        self
    }

    /// Set the conversation's work environment. The topbar chooses it and the
    /// list filters by it; the card's own use is the cloud after the directory,
    /// so a run on a worker reads as one wherever its card is listed (the
    /// Starred section included).
    pub fn with_workspace_routing(mut self, routing: impl Into<String>) -> Self {
        self.workspace_routing = routing.into();
        self
    }

    /// Set the working directory drawn under the card's subject. An empty path
    /// draws no row.
    pub fn with_directory(mut self, directory: impl Into<String>) -> Self {
        let directory = directory.into();
        self.directory = if directory.trim().is_empty() {
            None
        } else {
            Some(directory)
        };
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Set how far the 3-dot control sits from the card's trailing edge.
    ///
    /// The card does not know the surface it sits on, so a host that wants the
    /// dots a fixed distance from that surface's own edge passes the surface's
    /// padding subtracted from that distance. The sidebar is inset by its own
    /// horizontal padding, so its call is `5.0 - padding` and the dots land 5 pt
    /// inside the sidebar's width.
    pub fn with_dots_inset(mut self, inset: f32) -> Self {
        self.dots_inset = inset.max(0.0);
        self
    }

    /// Build the card's elements once, reflecting the current hover/menu state.
    /// It is called from `layout` (which has `app`); it is *not* rebuilt during
    /// `paint`/`dispatch`, so the sizes computed by `layout` are preserved.
    fn ensure_root(&mut self, app: &AppContext) {
        if self.row.is_some() {
            return;
        }
        let xs = 6.0_f32;

        let ui = self.ui.borrow();
        let hover = ui.hover;
        let menu_open = ui.menu_open;
        let highlight = self.menu_row(ui.highlight);

        // The band's own colour, which `paint` fills the whole card with (and
        // the fade blends into): the pointer over the card, the selected
        // conversation, or the surface the card sits on.
        self.bg = if hover {
            app.theme.color(ColorToken::Hover)
        } else {
            app.theme.color(ColorToken::Surface)
        };

        let mut subject = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Text::new(self.name.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            );
        if self.starred {
            subject = subject.with_child(
                Icon::new("star-filled")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        let name_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_spacing(xs)
            .with_child(subject.finish())
            .with_child(
                Text::new(self.timestamp.clone())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();

        // The harness mark, with the conversation's status as a badge hanging
        // off its bottom-right corner. The mark says what runs the work; the
        // badge says how the last turn ended: gray when there is nothing to
        // report, yellow while the agent is working or waiting on the user, red
        // when the last turn failed and green when it finished. The badge's
        // ring is the card's own fill, so it cuts a clean edge out of the mark
        // instead of tinting it.
        let mark = harness_mark(&self.harness, HARNESS_SIZE, app);
        let badge = Container::new(
            Empty::new()
                .with_size(vec2f(STATUS_BADGE_SIZE, STATUS_BADGE_SIZE))
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(self.status.color_token())))
        .with_border(Border::all(STATUS_BADGE_RING).with_border_color(self.bg))
        .with_corner_radius(STATUS_BADGE_SIZE / 2.0)
        .finish();
        let harness = Stack::new()
            .with_children([mark])
            .with_overlay(badge, vec2f(STATUS_BADGE_OFFSET, STATUS_BADGE_OFFSET))
            .finish();

        let title = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(HARNESS_GAP)
            .with_child(harness)
            .with_child(name_row)
            .finish();

        let mut text_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(title);
        // The directory the conversation works in, directly under its subject:
        // it says where the work happens, which the title alone cannot. It
        // keeps the title's inset, so the column reads as one block beside the
        // square's gutter.
        if let Some(directory) = self.directory.clone() {
            let mut row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .with_child(
                    Icon::new("folder")
                        .with_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new(directory)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .with_max_lines(1)
                        .finish(),
                );
            // A conversation that runs on a worker says so right after the
            // directory it works in: the path is the worker's, and the cloud
            // is what makes that readable at a glance. A local one adds
            // nothing.
            if self.workspace_routing == "remote" {
                row = row.with_child(
                    Icon::new("cloud")
                        .with_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
            text_column = text_column.with_child(beside_square(row.finish()));
        }
        let text_column = text_column.finish();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(Expanded::new(text_column).finish());

        // Square: the sidebar is a flat surface, so a card is a full-width band
        // rather than a rounded widget. Only the content is inset from the
        // band's edges: the band's fill is painted by `paint` and not by this
        // container, so the fill and the hit area stay full width — the row is
        // clipped clear of the dots while the band is not, so the two cannot
        // come from one clipped subtree.
        self.row = Some(
            Container::new(row.finish())
                .with_padding(crate::style::EdgeInsets::new(
                    CARD_CONTENT_INSET,
                    CARD_EXTRA_HEIGHT,
                    CARD_CONTENT_INSET,
                    CARD_EXTRA_HEIGHT,
                ))
                .finish(),
        );

        // The card's own tray: star the conversation or delete it, in a card
        // anchored to the right of the 3-dot control rather than in rows under
        // the row — the controls open their own trays beside themselves the same
        // way. It is laid out here and placed by `paint`, so it takes no room in
        // the list and opening it does not push the cards below it down.
        if menu_open {
            let mut rows = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(xs);
            if let Some(star) = self.star_row(app, highlight == STAR_ROW) {
                rows = rows.with_child(star);
            }
            rows = rows.with_child(self.delete_row(app, highlight == DELETE_ROW));
            let tray = Container::new(rows.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_border(app.theme.color(ColorToken::Border).into())
                .with_corner_radius(TRAY_RADIUS)
                .with_padding(EdgeInsets::new(0.0, TRAY_PADDING, 0.0, TRAY_PADDING))
                .finish();
            self.menu = Some(Rc::new(RefCell::new(tray)));
        }

        // 3-dot menu, visible on hover or while the delete menu is open.
        if hover || menu_open {
            let ui_dots = Rc::clone(&self.ui);
            let first_row = self.first_menu_row();
            let has_star = self.on_toggle_star.is_some();
            self.dots = Some(
                TopbarButton::new(
                    Icon::new("dots-horizontal")
                        .with_size(DOTS_SIZE)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_size(DOTS_SIZE)
                .with_corner_radius(0.0)
                .with_on_click(move || {
                    let mut ui = ui_dots.borrow_mut();
                    if ui.menu_open {
                        ui.close_menu();
                    } else {
                        ui.menu_open = true;
                        if !has_star || ui.highlight > DELETE_ROW {
                            ui.highlight = first_row;
                        }
                    }
                })
                .finish(),
            );
        }
    }

    /// The tray's first row, which is where opening it puts the highlight.
    fn first_menu_row(&self) -> usize {
        if self.on_toggle_star.is_some() {
            STAR_ROW
        } else {
            DELETE_ROW
        }
    }

    /// `index` clamped to a row this card actually draws: a card with no star
    /// action has only the delete row.
    fn menu_row(&self, index: usize) -> usize {
        if self.on_toggle_star.is_some() {
            index.min(DELETE_ROW)
        } else {
            DELETE_ROW
        }
    }

    /// The 3-dot control's box. It is pinned to the card's trailing edge —
    /// `dots_inset` in from it — and centred on the row's band, so neither the
    /// title's width nor the menu's height can move it. `None` while the card
    /// draws no control.
    ///
    /// The arithmetic the sidebar relies on: the card's trailing edge sits on
    /// the sidebar's own padding, and the control is `dots_inset` further in, so
    /// `sidebar_padding + dots_inset` is the gap between the dots and the
    /// sidebar's edge.
    fn dots_bounds(&self) -> Option<RectF> {
        let (origin, size) = (self.origin?, self.size?);
        self.dots.as_ref()?;
        Some(rectf(
            origin.x() + size.x - self.dots_inset - DOTS_SIZE,
            origin.y() + (self.row_height - DOTS_SIZE) / 2.0,
            DOTS_SIZE,
            DOTS_SIZE,
        ))
    }

    /// The tray's box: hung from the 3-dot control it belongs to, on the
    /// control's right side — a `TRAY_GAP` past the dots and top-aligned with
    /// them — so it opens into the space beside the sidebar instead of over the
    /// cards below. `None` until the tray has been laid out.
    fn menu_bounds(&self) -> Option<RectF> {
        let dots = self.dots_bounds()?;
        let menu_size = self.menu_size?;
        self.menu.as_ref()?;
        Some(rectf(
            dots.max_x() + TRAY_GAP,
            dots.min_y(),
            menu_size.x,
            menu_size.y,
        ))
    }

    /// Whether `position` is on the tray's surface — the tray's own box and the
    /// `TRAY_GAP`-wide strip between it and the control. The tray sits a few
    /// points right of the dots, so a pointer walking from the control into the
    /// tray crosses that strip: without it the card would read the crossing as
    /// the pointer leaving and put the tray away before it arrived.
    fn pointer_on_menu(&self, position: Vector2F) -> bool {
        let Some(bounds) = self.menu_bounds() else {
            return false;
        };
        let bridge = rectf(
            bounds.min_x() - TRAY_GAP,
            bounds.min_y(),
            TRAY_GAP,
            bounds.height(),
        );
        contains(bounds, position) || contains(bridge, position)
    }

    /// The pointer's position on a mouse event, or `None` for the events that
    /// carry no pointer (keys, focus, scroll).
    fn pointer_position(event: &DispatchedEvent) -> Option<Vector2F> {
        match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => Some(*position),
            _ => None,
        }
    }

    /// Whether the tray's own rows are the ones the event is for. The tray is
    /// anchored below the row and so lies outside the card's own band: gating on
    /// it is what keeps a press on the tray from reaching the row behind it (and
    /// keeps a pointer move on the tray from reading as a pointer that left the
    /// card). A non-pointer event is the tray's to answer: it holds the only
    /// focusable rows of the two.
    fn menu_takes_event(&self, event: &DispatchedEvent) -> bool {
        match Self::pointer_position(event) {
            None => true,
            Some(position) => self.pointer_on_menu(position),
        }
    }

    /// The area the row may paint in: the card's content box, which stops at the
    /// content inset on the trailing edge (the text is measured with nothing to
    /// wrap against, so the clip is what cuts it at that inset), and clear of
    /// the control's gutter while the control is drawn. It is the row's own band
    /// only, so the menu below it is not clipped with the row.
    fn row_clip(&self, origin: Vector2F, size: Vector2F) -> RectF {
        let content_right = size.x - CARD_CONTENT_INSET;
        let right = if self.dots.is_some() {
            content_right.min(size.x - self.dots_inset - DOTS_SIZE - DOTS_CLEARANCE)
        } else {
            content_right
        };
        rectf(origin.x, origin.y, right.max(0.0), self.row_height)
    }

    /// The menu's star row, or `None` when the card was given no star action.
    /// Starring closes the menu: the choice is made, and the card and the
    /// sidebar's Starred section already show it.
    fn star_row(&self, app: &AppContext, selected: bool) -> Option<Box<dyn Element>> {
        let on_toggle = self.on_toggle_star.clone()?;
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let starred = self.starred;
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Icon::new(if starred { "star-filled" } else { "star" })
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(if starred { "Unstar" } else { "Star" })
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        let ui = Rc::clone(&self.ui);
        Some(
            HoverRow::new(row)
                .with_padding(menu_row_padding(app))
                .with_selected(selected)
                .with_on_click(move || {
                    ui.borrow_mut().close_menu();
                    (on_toggle.borrow_mut())();
                })
                .finish(),
        )
    }

    fn delete_row(&self, app: &AppContext, selected: bool) -> Box<dyn Element> {
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let on_delete = self.on_delete.clone();
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Icon::new("trash")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Error, app)
                    .finish(),
            )
            .with_child(
                Text::new("Delete conversation")
                    .with_theme_color(ColorToken::Error, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        HoverRow::new(row)
            .with_padding(menu_row_padding(app))
            .with_selected(selected)
            .with_on_click(move || {
                if let Some(cb) = on_delete.as_ref() {
                    (cb.borrow_mut())();
                }
            })
            .finish()
    }

    /// The tray's keys, answered only while it is open: the arrows move the
    /// highlight between its rows, Enter runs the highlighted row and puts the
    /// tray away, and Escape puts it away. A key the tray does not take is left
    /// to the row.
    fn menu_key(&mut self, event: &DispatchedEvent) -> bool {
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if modifiers.command || modifiers.ctrl || modifiers.alt || !self.ui.borrow().menu_open {
            return false;
        }
        match key.as_str() {
            "ArrowUp" | "ArrowDown" => {
                let current = self.menu_row(self.ui.borrow().highlight);
                let first = self.first_menu_row();
                let next = if key == "ArrowDown" {
                    (current + 1).min(DELETE_ROW)
                } else {
                    current.saturating_sub(1).max(first)
                };
                self.ui.borrow_mut().highlight = next;
                true
            }
            "Enter" => {
                let highlighted = self.menu_row(self.ui.borrow().highlight);
                self.run_menu_row(highlighted);
                true
            }
            "Escape" => {
                self.ui.borrow_mut().close_menu();
                true
            }
            _ => false,
        }
    }

    /// Run the action of the tray's row `index` and put the tray away: the
    /// keyboard's Enter, which chooses a row the pointer never touched.
    fn run_menu_row(&self, index: usize) {
        self.ui.borrow_mut().close_menu();
        let callback = if index == STAR_ROW {
            self.on_toggle_star.clone()
        } else {
            self.on_delete.clone()
        };
        if let Some(cb) = callback {
            (cb.borrow_mut())();
        }
    }
}

/// A row of the card's text column under the title, inset to the title's own x.
/// The title starts after the status square's gutter ([`TEXT_GUTTER`], on top of
/// the content inset its container already gives it); the rows under it line up
/// with it rather than with the square's edge.
fn beside_square(row: Box<dyn Element>) -> Box<dyn Element> {
    Padding::new(row, EdgeInsets::new(TEXT_GUTTER, 0.0, 0.0, 0.0)).finish()
}

/// The inset a tray row gives its content. The tray has no horizontal padding
/// of its own, so the band a row paints spans the tray edge to edge; the label
/// keeps the inset the ghost button gave it — the tray's padding plus the
/// control's own horizontal padding.
fn menu_row_padding(app: &AppContext) -> EdgeInsets {
    let horizontal = TRAY_PADDING + app.theme.spacing_px(SpacingToken::Md);
    let vertical = app.theme.spacing_px(SpacingToken::Sm);
    EdgeInsets::new(horizontal, vertical, horizontal, vertical)
}

/// The width a DSH sidebar context-menu card takes: its widest row's natural
/// width, held between DSH's 144pt menu minimum and the sidebar's own budget.
/// DSH's `.list` is `width: max-content` between those bounds, so a two-word
/// workspace menu stays narrow while a long session row grows the card.
fn sidebar_menu_width(items: &[SidebarMenuItem]) -> f32 {
    let widest = items
        .iter()
        .map(|item| {
            let label = measure_text(
                &item.label,
                SIDEBAR_MENU_LABEL_SIZE,
                SIDEBAR_MENU_LABEL_LINE,
                f32::INFINITY,
            )
            .x;
            SIDEBAR_MENU_ROW_PADDING_X * 2.0
                + SIDEBAR_MENU_ICON_SIZE
                + SIDEBAR_MENU_ICON_GAP
                + label
        })
        .fold(0.0_f32, f32::max);
    (widest + 2.0 * SIDEBAR_MENU_PADDING).clamp(SIDEBAR_MENU_MIN_WIDTH, SIDEBAR_MENU_MAX_WIDTH)
}

/// One row of a DSH sidebar context-menu card: a 14pt leading icon, a 13/20
/// label, a 34pt row band with a 12pt radius, the label in `Text`, the icon in
/// `Muted`, and both in `Error` for a destructive row (DSH's `.danger`). The
/// band's fill is the visible `Selected` step on hover and for the keyboard
/// highlight, DSH's single interactive-hover fill.
fn sidebar_menu_row<F>(
    app: &AppContext,
    item: &SidebarMenuItem,
    selected: bool,
    on_activate: F,
) -> Box<dyn Element>
where
    F: FnMut() + 'static,
{
    let (label_ink, icon_ink) = if item.danger {
        (ColorToken::Error, ColorToken::Error)
    } else {
        (ColorToken::Text, ColorToken::Muted)
    };
    let vertical = (SIDEBAR_MENU_ROW_HEIGHT - SIDEBAR_MENU_LABEL_LINE) / 2.0;
    let content = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(SIDEBAR_MENU_ICON_GAP)
        .with_child(
            Icon::new(item.icon)
                .with_size(SIDEBAR_MENU_ICON_SIZE)
                .with_theme_color(icon_ink, app)
                .finish(),
        )
        .with_child(
            Expanded::new(
                Text::new(item.label.clone())
                    .with_font_size(SIDEBAR_MENU_LABEL_SIZE)
                    .with_line_height(SIDEBAR_MENU_LABEL_LINE / SIDEBAR_MENU_LABEL_SIZE)
                    .with_max_lines(1)
                    .with_theme_color(label_ink, app)
                    .finish(),
            )
            .finish(),
        )
        .finish();
    HoverRow::new(content)
        .with_padding(EdgeInsets::new(
            SIDEBAR_MENU_ROW_PADDING_X,
            vertical,
            SIDEBAR_MENU_ROW_PADDING_X,
            vertical,
        ))
        .with_selected(selected)
        .with_hover_token(ColorToken::Selected)
        .with_corner_radius(SIDEBAR_MENU_ROW_RADIUS)
        .with_on_click(on_activate)
        .finish()
}

/// The casing of a DSH sidebar context-menu: the raised surface with a hairline
/// border, the 16pt card radius and the 4pt padding that surrounds the rows.
fn sidebar_menu_tray(
    app: &AppContext,
    rows: Box<dyn Element>,
    items: &[SidebarMenuItem],
) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(rows)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .with_corner_radius(SIDEBAR_MENU_RADIUS)
            .with_padding(EdgeInsets::uniform(SIDEBAR_MENU_PADDING))
            .finish(),
    )
    .with_width(sidebar_menu_width(items))
    .finish()
}

/// The hairline between two groups of a DSH sidebar context menu: a 0.5pt rule
/// with 3pt above and below, inset 8pt to the row content (12pt inside the
/// card once the casing's own padding is counted).
struct SidebarTraySeparator {
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for SidebarTraySeparator {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() {
            constraint.max.x
        } else {
            0.0
        };
        let size = vec2f(
            width,
            SIDEBAR_MENU_SEPARATOR_HEIGHT + 2.0 * SIDEBAR_MENU_SEPARATOR_MARGIN,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(
                rectf(
                    origin.x + SIDEBAR_MENU_SEPARATOR_INSET,
                    origin.y + SIDEBAR_MENU_SEPARATOR_MARGIN,
                    (size.x - 2.0 * SIDEBAR_MENU_SEPARATOR_INSET).max(0.0),
                    SIDEBAR_MENU_SEPARATOR_HEIGHT,
                ),
                app.theme.color(ColorToken::Border),
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

impl Element for ConversationListItem {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let row = self.row.as_mut().unwrap().layout(constraint, ctx, app);
        self.row_height = row.y;
        // The card is the row: the tray is anchored to the control and takes no
        // room, so opening it never moves the cards around it.
        let size = row;
        self.menu_size = None;
        if let Some(menu) = self.menu.as_ref() {
            let menu_size = menu.borrow_mut().layout(
                SizeConstraint::loose(vec2f(TRAY_MAX_WIDTH, 400.0)),
                ctx,
                app,
            );
            self.menu_size = Some(menu_size);
        }
        // The control's box is fixed, so it is laid out tight and placed by
        // `paint` rather than by any parent's cursor.
        if let Some(dots) = self.dots.as_mut() {
            let _ = dots.layout(SizeConstraint::tight(vec2f(DOTS_SIZE, DOTS_SIZE)), ctx, app);
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };

        // The card's band, then everything inside it clipped to the card so a
        // long one-line title or path is cut at the card's right edge instead of
        // spilling into the main area.
        let card = rectf(origin.x, origin.y, size.x, size.y);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(card, self.bg, 0.0);
            renderer.clip_rect(card);
        }

        // The row stops at the content's trailing edge, and short of the control
        // too while the control is drawn, so the title is cut at the inset rather
        // than painted over the band's edge or under the dots. The menu and the
        // control are painted past that clip — inside the card's own, which
        // keeps them within the surface they belong to.
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.clip_rect(self.row_clip(origin, size));
        }
        if let Some(row) = self.row.as_mut() {
            row.paint(origin, ctx, app);
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }
        if let Some(bounds) = self.dots_bounds() {
            if let Some(dots) = self.dots.as_mut() {
                dots.paint(vec2f(bounds.min_x(), bounds.min_y()), ctx, app);
            }
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }

        // The tray, in its own card below the control. It is painted here so a
        // caller that paints the card alone still draws its tray, and queued
        // into the frame's last paint layer with the same element so the cards
        // painted after this one cannot cover it: the two paints are the same
        // card at the same place, and the second one is the frame's last.
        if let Some(bounds) = self.menu_bounds() {
            if let Some(menu) = self.menu.as_ref() {
                let at = vec2f(bounds.min_x(), bounds.min_y());
                menu.borrow_mut().paint(at, ctx, app);
                app.hover_chips
                    .borrow_mut()
                    .push_shared(at, Rc::clone(menu));
            }
        }

        // Fade the trailing edge of the content until the 3-dot menu appears
        // (the row is then clipped clear of the dots). It ends on the content's
        // own edge, so the cut the row's clip makes there is a fade rather than
        // a hard line. Skip while the menu is open so the fade never covers the
        // delete button.
        let ui = self.ui.borrow();
        if !ui.hover && !ui.menu_open {
            let fade_w = 24.0_f32;
            let content_width = size.x - CARD_CONTENT_INSET;
            if content_width > fade_w {
                let rect = rectf(origin.x + content_width - fade_w, origin.y, fade_w, size.y);
                if let Some(renderer) = ctx.renderer.as_mut() {
                    renderer.fill_rect_fade_right(rect, self.bg, 0.0);
                }
            }
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.ensure_root(app);
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        // The tray's keys are its own while it is open; every other key and
        // every key while it is closed reaches the row.
        if self.menu_key(event) {
            return true;
        }

        // Let the 3-dot control first (it is painted over the row and its
        // gutter is its own), then the tray's rows, then the row: otherwise the
        // card itself would swallow the click. The tray is anchored outside the
        // card's band, so only the events that land on it go to it.
        if let Some(dots) = self.dots.as_mut() {
            if dots.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        let takes_menu = self.menu_takes_event(event);
        if let Some(menu) = self.menu.as_mut() {
            if takes_menu && menu.borrow_mut().dispatch_event(event, ctx, app) {
                return true;
            }
        }
        if let Some(row) = self.row.as_mut() {
            if row.dispatch_event(event, ctx, app) {
                return true;
            }
        }

        if let DispatchedEvent::MouseMove { position } = event {
            // A pointer on the tray is still on the card: the tray is the
            // control's own surface, and reading it as "left the card" would put
            // the tray away as soon as the pointer reached it.
            let inside = contains(bounds, *position) || self.menu_takes_event(event);
            let mut ui = self.ui.borrow_mut();
            ui.hover = inside;
            if !inside {
                ui.close_menu();
            }
            return false;
        }

        let on_select = self.on_select.clone();
        let mut select = move || {
            if let Some(cb) = on_select.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut select)
    }
}

/// A small helper used by tests to find a card's hover bounds.
pub fn card_bounds(item: &ConversationListItem) -> Option<RectF> {
    item.bounds()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;

    #[test]
    fn conversation_list_item_layouts() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut item = ConversationListItem::new("c1", "Ada", "40 min ago", ui, false);
        let size = item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn conversation_list_item_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut item = ConversationListItem::new("c1", "Ada", "5 min ago", ui, false)
            .with_on_click(move || *clicked_clone.borrow_mut() = true);

        item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        item.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(item.dispatch_event(&down, &mut event_ctx, &app));
        assert!(item.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }

    #[test]
    fn hover_updates_shared_state() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut item = ConversationListItem::new("c1", "Ada", "5 min ago", Rc::clone(&ui), false);
        item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        item.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        item.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(10.0, 10.0),
            },
            &mut event_ctx,
            &app,
        );
        assert!(ui.borrow().hover);
    }

    #[test]
    fn fade_emits_gradient_command() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let item = ConversationListItem::new(
            "c1",
            "A subject far wider than the card it is listed in fades out",
            "5 min ago",
            ui,
            false,
        );
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
        let counts = crate::test_util::command_counts(&commands);
        assert_eq!(
            counts.fill_rect_fade, 1,
            "idle card should emit a right fade"
        );
        assert!(counts.draw_text > 0, "card should render text");
    }

    /// A conversation card leads with its harness mark, and the status is a
    /// badge hanging off that mark's bottom-right corner rather than a square
    /// of its own. The badge has to overhang the mark by
    /// [`STATUS_BADGE_OVERHANG`], carry the status colour, and keep a ring in
    /// the card's own background so it cuts the mark's corner cleanly.
    #[test]
    fn the_card_leads_with_the_harness_mark_and_badges_its_status_on_the_corner() {
        let app = AppContext::default();
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "5 min ago",
            Rc::new(RefCell::new(AgentCardUi::default())),
            false,
        )
        .with_harness("claude")
        .with_status(ConversationStatus::Waiting);
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let (mark_origin, mark_size) = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, name, size, ..
                } if name == "harness-claude" => Some((*origin, *size)),
                _ => None,
            })
            .expect("the card draws the Claude mark");
        assert_eq!(mark_size, HARNESS_SIZE);

        let warning = app.theme.color(ColorToken::Warning);
        let badge = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == warning
                    && (*corner_radius - STATUS_BADGE_SIZE / 2.0).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the waiting status draws its badge");
        assert!(
            (badge.min_x() - (mark_origin.x + STATUS_BADGE_OFFSET)).abs() < 0.01
                && (badge.min_y() - (mark_origin.y + STATUS_BADGE_OFFSET)).abs() < 0.01,
            "the badge sits on the mark's corner: badge {badge:?}, mark {mark_origin:?}"
        );
        assert!(
            badge.max_x() > mark_origin.x + HARNESS_SIZE
                && badge.max_y() > mark_origin.y + HARNESS_SIZE,
            "the badge overhangs the mark's corner: badge {badge:?}"
        );

        let ring = commands.iter().any(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } => {
                (*color == app.theme.color(ColorToken::Surface))
                    && (*width - STATUS_BADGE_RING).abs() < 0.01
                    && (*corner_radius - STATUS_BADGE_SIZE / 2.0).abs() < 0.01
                    && (rect.min_x() - badge.min_x()).abs() < 0.01
            }
            _ => false,
        });
        assert!(ring, "the badge keeps a ring in the card's own background");
    }

    #[test]
    fn harness_keys_map_to_their_marks() {
        assert_eq!(harness_icon("claude"), "harness-claude");
        assert_eq!(harness_icon("anthropic"), "harness-claude");
        assert_eq!(harness_icon("codex"), "harness-codex");
        assert_eq!(harness_icon("OpenAI"), "harness-codex");
        assert_eq!(harness_icon("xai"), "harness-grok");
        assert_eq!(harness_icon("gemini"), "harness-gemini");
        assert_eq!(harness_icon("goble"), "harness-goble");
        assert_eq!(harness_icon("ollama"), "harness-goble");
    }

    /// The 3-dot control's box and colour, from the icon it paints.
    fn dots_box(commands: &[RenderCommand]) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name, origin, size, ..
            } if name == "dots-horizontal" => Some(rectf(origin.x, origin.y, *size, *size)),
            _ => None,
        })
    }

    /// The dots are pinned to the card's trailing edge: a title wider than the
    /// card — the card's text is one line, measured with nothing to wrap
    /// against — cannot carry them off it, and the row is clipped clear of them
    /// so the title cannot paint under them either.
    #[test]
    fn the_dots_are_pinned_to_the_trailing_edge_clear_of_the_title() {
        let app = AppContext::default();
        let long_title = "A conversation whose subject is far wider than the card it is listed in";

        // An idle card draws no control at all: there is nothing to anchor.
        let idle = ConversationListItem::new(
            "c1",
            long_title,
            "5 min ago",
            Rc::new(RefCell::new(AgentCardUi::default())),
            false,
        );
        let mut element: Box<dyn Element> = Box::new(idle);
        let width = 200.0;
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);
        assert!(dots_box(&commands).is_none(), "an idle card draws no dots");
        let idle_clips: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::ClipRect(rect) => Some(*rect),
                _ => None,
            })
            .collect();
        assert!(
            idle_clips
                .iter()
                .all(|clip| clip.max_x() >= width - CARD_CONTENT_INSET - 0.01),
            "an idle card needs no gutter clip: nothing cuts inside the content box {idle_clips:?}"
        );
        assert!(
            idle_clips
                .iter()
                .any(|clip| (clip.max_x() - (width - CARD_CONTENT_INSET)).abs() < 0.01),
            "and cuts its row at the content's edge: {idle_clips:?}"
        );

        // The hovered card pins its control to its trailing edge, inset by the
        // gap it was given.
        let hovered = ConversationListItem::new(
            "c1",
            long_title,
            "5 min ago",
            Rc::new(RefCell::new(AgentCardUi {
                hover: true,
                menu_open: false,
                ..Default::default()
            })),
            false,
        )
        .with_dots_inset(1.0);
        let mut element: Box<dyn Element> = Box::new(hovered);
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);

        let dots = dots_box(&commands).expect("a hovered card draws its dots");
        assert_eq!(
            dots.width(),
            DOTS_SIZE,
            "the control's box is the glyph's box"
        );
        assert_eq!(
            dots.max_x(),
            width - 1.0,
            "the control is inset from the card's own trailing edge: {dots:?}"
        );

        // The same card with a subject that fits draws its control in the same
        // place: the title's width is not what positions it.
        let narrow = ConversationListItem::new(
            "c1",
            "Ada",
            "5 min ago",
            Rc::new(RefCell::new(AgentCardUi {
                hover: true,
                menu_open: false,
                ..Default::default()
            })),
            false,
        )
        .with_dots_inset(1.0);
        let mut element: Box<dyn Element> = Box::new(narrow);
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);
        assert_eq!(
            dots_box(&commands).map(|dots| dots.max_x()),
            Some(width - 1.0),
            "a title that fits does not change where the control sits"
        );

        // The narrowest clip in the frame is the row's, and it stops short of
        // the control over the control's own band.
        let row_clip = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::ClipRect(rect) => Some(*rect),
                _ => None,
            })
            .min_by(|a, b| a.max_x().total_cmp(&b.max_x()))
            .expect("the card paints its content clipped");
        assert!(
            row_clip.max_x() <= dots.min_x(),
            "the row is clipped clear of the dots: {row_clip:?} vs {dots:?}"
        );
        assert!(
            row_clip.min_y() <= dots.min_y() && row_clip.max_y() >= dots.max_y(),
            "and the clip covers the control's band: {row_clip:?}"
        );
    }

    /// The hovered card draws its 3-dot control, and an open menu draws its
    /// actions in a card of its own: beside the dots, opening into the space
    /// right of the sidebar rather than over the cards below, and wide enough
    /// that its rows span the tray.
    #[test]
    fn hover_renders_dots_and_menu_renders_delete() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            hover: true,
            menu_open: true,
            ..Default::default()
        }));
        let mut item = ConversationListItem::new("c1", "Ada", "5 min ago", ui, false)
            .with_on_toggle_star(|| {});
        let card = item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(crate::render::Renderer::new());
        item.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default();

        let has_dots = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "dots-horizontal")
        });
        let has_star = commands.iter().any(
            |c| matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text == "Star"),
        );
        let has_delete = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text == "Delete conversation")
        });
        assert!(has_dots, "hover/menu card should render the 3-dot icon");
        assert!(has_star, "open menu should render a 'Star' action");
        assert!(
            has_delete,
            "open menu should render a 'Delete conversation' action"
        );

        let dots = item.dots_bounds().expect("the menu draws the control");
        let tray = item
            .menu_bounds()
            .expect("the open menu is placed beside the control");
        assert!(
            tray.min_x() >= dots.max_x(),
            "the tray opens to the right of the dots: {tray:?} vs dots {dots:?}"
        );
        assert!(
            (tray.min_y() - dots.min_y()).abs() < 0.01,
            "the tray is anchored to the dots' top: {tray:?} vs dots {dots:?}"
        );
        assert!(
            tray.width() >= TRAY_MAX_WIDTH - 0.01,
            "the tray takes its full width so its rows span it: {tray:?}"
        );
        assert!(
            tray.max_x() > card.x,
            "the tray opens into the space beside the sidebar: {tray:?} vs card {card:?}"
        );
    }

    /// The card's menu carries the star toggle, and clicking it stars the
    /// conversation and puts the menu away: the card and the sidebar's Starred
    /// section are where the choice now shows.
    #[test]
    fn the_menus_star_row_stars_the_conversation_and_closes_the_menu() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            hover: true,
            menu_open: true,
            ..Default::default()
        }));
        let toggled = Rc::new(RefCell::new(0usize));
        let ran = Rc::clone(&toggled);
        let item = ConversationListItem::new("c1", "Ada", "5 min ago", Rc::clone(&ui), false)
            .with_on_toggle_star(move || *ran.borrow_mut() += 1);
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let offers_star = commands.iter().any(|command| {
            matches!(command, crate::render::RenderCommand::DrawText { text, .. } if text == "Star")
        });
        assert!(offers_star, "the open menu should offer Star");

        let origin = commands
            .iter()
            .find_map(|command| match command {
                crate::render::RenderCommand::DrawIcon { name, origin, .. } if name == "star" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the star row's icon is drawn");
        let at = origin + vec2f(7.0, 7.0);
        let mut event_ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            element.dispatch_event(&event, &mut event_ctx, &app);
        }

        assert_eq!(*toggled.borrow(), 1, "the star row ran the toggle");
        assert!(!ui.borrow().menu_open, "starring closes the menu");
    }

    /// A key press the way the window sends it.
    fn press(key: &str) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: key.to_string(),
            modifiers: crate::event::ModifiersState::none(),
        }
    }

    /// A card with both tray actions, laid out and painted so it has bounds to
    /// dispatch keys into. The counters are the two actions' runs.
    fn open_tray_card(
        app: &AppContext,
        ui: AgentCardUi,
    ) -> (
        Box<dyn Element>,
        Rc<RefCell<AgentCardUi>>,
        Rc<RefCell<usize>>,
        Rc<RefCell<usize>>,
    ) {
        let ui = Rc::new(RefCell::new(ui));
        let stars = Rc::new(RefCell::new(0usize));
        let deletes = Rc::new(RefCell::new(0usize));
        let on_star = Rc::clone(&stars);
        let on_delete = Rc::clone(&deletes);
        let item = ConversationListItem::new("c1", "Ada", "5 min ago", Rc::clone(&ui), false)
            .with_on_toggle_star(move || *on_star.borrow_mut() += 1)
            .with_on_delete(move || *on_delete.borrow_mut() += 1);
        let mut element: Box<dyn Element> = Box::new(item);
        let _ = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), app);
        (element, ui, stars, deletes)
    }

    /// The tray's own rounded card, around its rows.
    fn tray_box(commands: &[RenderCommand], app: &AppContext) -> RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && *corner_radius > 0.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the open tray draws its card")
    }

    /// The square band a row paints: the tray's selection, or its hover.
    fn row_band(commands: &[RenderCommand], color: crate::color::ColorU) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color: fill,
                corner_radius,
            } if *fill == color && *corner_radius == 0.0 => Some(*rect),
            _ => None,
        })
    }

    fn icon_box(commands: &[RenderCommand], name: &str) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name: icon,
                origin,
                size,
                ..
            } if icon == name => Some(rectf(origin.x, origin.y, *size, *size)),
            _ => None,
        })
    }

    /// Paint the card with the pointer at `cursor`, so its tray rows answer
    /// hover the way they do in the window.
    fn paint_with_cursor(
        element: &mut Box<dyn Element>,
        app: &AppContext,
        cursor: Vector2F,
    ) -> Vec<RenderCommand> {
        let _ = element.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(crate::render::Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = cursor;
        element.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    #[test]
    fn the_trays_arrows_move_the_highlight_between_its_rows() {
        let app = AppContext::default();
        let (mut element, ui, _, _) = open_tray_card(
            &app,
            AgentCardUi {
                menu_open: true,
                ..Default::default()
            },
        );
        let mut ctx = EventContext::default();

        assert!(element.dispatch_event(&press("ArrowDown"), &mut ctx, &app));
        assert_eq!(ui.borrow().highlight, DELETE_ROW, "Down moves to delete");
        assert!(element.dispatch_event(&press("ArrowDown"), &mut ctx, &app));
        assert_eq!(
            ui.borrow().highlight,
            DELETE_ROW,
            "the highlight stops at the last row"
        );
        assert!(element.dispatch_event(&press("ArrowUp"), &mut ctx, &app));
        assert_eq!(ui.borrow().highlight, STAR_ROW, "Up moves back to star");
        assert!(element.dispatch_event(&press("ArrowUp"), &mut ctx, &app));
        assert_eq!(
            ui.borrow().highlight,
            STAR_ROW,
            "and stops at the first row"
        );
        assert!(ui.borrow().menu_open, "moving the highlight leaves it open");
    }

    #[test]
    fn the_tray_answers_its_keys_only_while_it_is_open() {
        let app = AppContext::default();
        let (mut element, ui, stars, deletes) = open_tray_card(&app, AgentCardUi::default());
        let mut ctx = EventContext::default();

        for key in ["ArrowUp", "ArrowDown", "Enter", "Escape"] {
            assert!(
                !element.dispatch_event(&press(key), &mut ctx, &app),
                "{key} is left to the card while the tray is closed"
            );
        }
        assert_eq!(ui.borrow().highlight, STAR_ROW);
        assert_eq!(*stars.borrow(), 0, "Enter runs no row while it is closed");
        assert_eq!(*deletes.borrow(), 0);
    }

    #[test]
    fn enter_runs_the_highlighted_star_row_and_closes_the_tray() {
        let app = AppContext::default();
        let (mut element, ui, stars, deletes) = open_tray_card(
            &app,
            AgentCardUi {
                menu_open: true,
                ..Default::default()
            },
        );
        let mut ctx = EventContext::default();

        assert!(element.dispatch_event(&press("Enter"), &mut ctx, &app));
        assert_eq!(*stars.borrow(), 1, "Enter ran Star");
        assert_eq!(*deletes.borrow(), 0);
        assert!(!ui.borrow().menu_open, "and put the tray away");
        assert_eq!(
            ui.borrow().highlight,
            STAR_ROW,
            "closing resets the highlight for the next open"
        );
    }

    #[test]
    fn enter_runs_the_highlighted_delete_row_and_closes_the_tray() {
        let app = AppContext::default();
        let (mut element, ui, stars, deletes) = open_tray_card(
            &app,
            AgentCardUi {
                menu_open: true,
                highlight: DELETE_ROW,
                ..Default::default()
            },
        );
        let mut ctx = EventContext::default();

        assert!(element.dispatch_event(&press("Enter"), &mut ctx, &app));
        assert_eq!(*deletes.borrow(), 1, "Enter ran Delete conversation");
        assert_eq!(*stars.borrow(), 0);
        assert!(!ui.borrow().menu_open, "and put the tray away");
        assert_eq!(ui.borrow().highlight, STAR_ROW);
    }

    #[test]
    fn escape_closes_the_tray_and_resets_the_highlight() {
        let app = AppContext::default();
        let (mut element, ui, stars, deletes) = open_tray_card(
            &app,
            AgentCardUi {
                menu_open: true,
                highlight: DELETE_ROW,
                ..Default::default()
            },
        );
        let mut ctx = EventContext::default();

        assert!(element.dispatch_event(&press("Escape"), &mut ctx, &app));
        assert!(!ui.borrow().menu_open);
        assert_eq!(ui.borrow().highlight, STAR_ROW);
        assert_eq!(*stars.borrow(), 0, "Escape runs neither row");
        assert_eq!(*deletes.borrow(), 0);
    }

    /// The highlighted row's band spans the tray's full inner width, with no
    /// padding around it, while its icon keeps the row's own inset. The row
    /// under the pointer paints the same band.
    #[test]
    fn the_trays_highlight_band_spans_the_tray_edge_to_edge() {
        let app = AppContext::default();
        let (mut element, ui, _, _) = open_tray_card(
            &app,
            AgentCardUi {
                hover: true,
                menu_open: true,
                ..Default::default()
            },
        );
        assert_eq!(ui.borrow().highlight, STAR_ROW);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let tray = tray_box(&commands, &app);
        let band = row_band(&commands, app.theme.color(ColorToken::Selected))
            .expect("the highlighted row paints a selection band");
        assert!(
            (band.min_x() - tray.min_x()).abs() < 0.01
                && (band.max_x() - tray.max_x()).abs() < 0.01,
            "the band spans the tray edge to edge: band {band:?} tray {tray:?}"
        );
        assert!(
            (band.width() - tray.width()).abs() < 0.01,
            "and no inset narrows it: band {band:?} tray {tray:?}"
        );

        let icon = icon_box(&commands, "star").expect("the star row draws its icon");
        let inset = TRAY_PADDING + app.theme.spacing_px(SpacingToken::Md);
        assert!(
            (icon.min_x() - band.min_x() - inset).abs() < 0.01,
            "the row's label keeps its horizontal inset: icon {icon:?} band {band:?}"
        );

        // The pointer's row: the same full-bleed band, in the hover colour. A
        // card that is not hovered keeps its own band out of the way.
        let delete = icon_box(&commands, "trash-02").expect("the delete row draws its icon");
        let cursor = vec2f(delete.min_x() + 2.0, delete.min_y() + 2.0);
        let (mut hovered, _, _, _) = open_tray_card(
            &app,
            AgentCardUi {
                menu_open: true,
                ..Default::default()
            },
        );
        let commands = paint_with_cursor(&mut hovered, &app, cursor);
        let hover = row_band(&commands, app.theme.color(ColorToken::Hover))
            .expect("the row under the pointer paints its hover band");
        assert!(
            (hover.min_x() - tray.min_x()).abs() < 0.01
                && (hover.max_x() - tray.max_x()).abs() < 0.01,
            "the hovered row uses the same geometry: hover {hover:?} tray {tray:?}"
        );
    }

    /// The tray opens on its first row unless the shared state already names a
    /// row it draws, and closing puts the highlight back on row 0 for the next
    /// open.
    #[test]
    fn opening_the_tray_keeps_a_valid_highlight_and_closing_resets_it() {
        let app = AppContext::default();
        let (mut element, ui, _, _) = open_tray_card(
            &app,
            AgentCardUi {
                hover: true,
                highlight: DELETE_ROW,
                ..Default::default()
            },
        );
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
        let dots = icon_box(&commands, "dots-horizontal").expect("a hovered card draws its dots");
        let at = vec2f(
            dots.min_x() + dots.width() / 2.0,
            dots.min_y() + dots.height() / 2.0,
        );
        let click = DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        };
        let mut ctx = EventContext::default();

        assert!(element.dispatch_event(&click, &mut ctx, &app));
        assert!(ui.borrow().menu_open);
        assert_eq!(
            ui.borrow().highlight,
            DELETE_ROW,
            "a valid highlight survives the open"
        );

        assert!(element.dispatch_event(&click, &mut ctx, &app));
        assert!(!ui.borrow().menu_open);
        assert_eq!(
            ui.borrow().highlight,
            STAR_ROW,
            "closing resets the highlight to row 0"
        );

        assert!(element.dispatch_event(&click, &mut ctx, &app));
        assert_eq!(
            ui.borrow().highlight,
            STAR_ROW,
            "and the next open starts there"
        );
    }

    /// The status badge the card paints: its box and its fill. It is the one
    /// small rounded fill on the card — the badge's ring is a stroke, not a
    /// fill, and the band's own fill has no radius.
    fn status_badge(commands: &[RenderCommand]) -> Option<(RectF, crate::color::ColorU)> {
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if rect.width() == STATUS_BADGE_SIZE
                && rect.height() == STATUS_BADGE_SIZE
                && (*corner_radius - STATUS_BADGE_SIZE / 2.0).abs() < 0.01 =>
            {
                Some((*rect, *color))
            }
            _ => None,
        })
    }

    /// The harness mark's box: the atlas glyph for a provider, or the
    /// full-colour Goble artwork the card draws as an image.
    fn harness_box(commands: &[RenderCommand]) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name, origin, size, ..
            } if name.starts_with("harness-") => Some(rectf(origin.x, origin.y, *size, *size)),
            RenderCommand::DrawImage { rect, source, .. } if source == "harness-goble-mark" => {
                Some(*rect)
            }
            _ => None,
        })
    }

    /// The box a drawn run occupies: its origin and the height it measured to,
    /// which is the band its line is centred in.
    fn run_box(commands: &[RenderCommand], run: &str) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText {
                text,
                origin,
                font_size,
                line_height,
                max_width,
                ..
            } if text == run => {
                let size =
                    crate::elements::text::measure_text(text, *font_size, *line_height, *max_width);
                Some(rectf(origin.x, origin.y, size.x, size.y))
            }
            _ => None,
        })
    }

    /// Every conversation's card leads with its harness mark, and the status is
    /// an 8x8 badge on that mark's bottom-right corner: gray when idle or
    /// stopped, yellow while the agent works or waits, red for a failed turn and
    /// green for a finished one.
    #[test]
    fn the_card_badges_its_status_on_the_harness_marks_corner() {
        let app = AppContext::default();

        for (status, token) in [
            (ConversationStatus::Default, ColorToken::Muted),
            (ConversationStatus::Stopped, ColorToken::Muted),
            (ConversationStatus::Waiting, ColorToken::Warning),
            (ConversationStatus::Error, ColorToken::Error),
            (ConversationStatus::Success, ColorToken::Success),
        ] {
            let item = ConversationListItem::new(
                "c1",
                "Ada",
                "40 min ago",
                Rc::new(RefCell::new(AgentCardUi::default())),
                false,
            )
            .with_status(status);
            let mut element: Box<dyn Element> = Box::new(item);
            let commands =
                crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

            let (badge, color) =
                status_badge(&commands).unwrap_or_else(|| panic!("{status:?}: no badge"));
            assert_eq!(
                (badge.width(), badge.height()),
                (STATUS_BADGE_SIZE, STATUS_BADGE_SIZE)
            );
            assert_eq!(color, app.theme.color(token), "{status:?} colour");
            assert_eq!(
                badge.min_x(),
                CARD_CONTENT_INSET + STATUS_BADGE_OFFSET,
                "{status:?}: the badge hangs off the mark's corner"
            );
            assert!(
                badge.max_x() > CARD_CONTENT_INSET + HARNESS_SIZE,
                "{status:?}: the badge overhangs the mark: {badge:?}"
            );
            let mark = harness_box(&commands).expect("the card draws its harness mark");
            assert_eq!(mark.width(), HARNESS_SIZE, "{status:?}: mark size");
            assert_eq!(mark.min_x(), CARD_CONTENT_INSET, "{status:?}: mark inset");
            assert!(
                !commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::DrawIcon { name, .. } if name.starts_with("conversation-")
                )),
                "{status:?}: the environment icon is not drawn any more"
            );
        }
    }

    /// The harness mark belongs to the title line: its centre is the subject's
    /// centre. A mark centred on the card's whole text column instead lands
    /// between the title and the path under it, which is the placement this
    /// pins against; the badge then hangs off the mark's corner.
    #[test]
    fn the_harness_mark_sits_on_the_title_line() {
        let app = AppContext::default();
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "40 min ago",
            Rc::new(RefCell::new(AgentCardUi::default())),
            false,
        )
        .with_directory("~/Projects/goble");
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
        let card = element.size().expect("the card laid out");

        let mark = harness_box(&commands).expect("the card draws its harness mark");
        let title = run_box(&commands, "Ada").expect("the card draws its subject");
        let path = run_box(&commands, "~/Projects/goble").expect("the card draws its path");

        let centre = |box_: RectF| box_.min_y() + box_.height() / 2.0;
        assert!(
            (centre(mark) - centre(title)).abs() <= 0.5,
            "the mark is centred on the title: mark {mark:?} title {title:?}"
        );
        assert!(
            (centre(mark) - card.y / 2.0).abs() > 1.0,
            "and no longer on the whole card: mark {mark:?} card {card:?}"
        );
        assert!(
            centre(title) < centre(path),
            "the path sits under the title: title {title:?} path {path:?}"
        );
        // The mark keeps its size, and the title starts past its gutter: the
        // content's inset, then the mark, then the gap.
        assert_eq!(mark.width(), HARNESS_SIZE);
        assert_eq!(
            title.min_x(),
            CARD_CONTENT_INSET + HARNESS_SIZE + HARNESS_GAP,
            "the subject's first character starts past the mark's gutter"
        );
        assert_eq!(
            title.min_x(),
            TEXT_INSET,
            "which is the one inset the card's rows are placed from"
        );
        assert_eq!(
            icon_box(&commands, "folder").map(|icon| icon.min_x()),
            Some(TEXT_INSET),
            "and the path row under it starts at the same inset"
        );
    }

    /// The card draws its subject and, when the conversation has a directory,
    /// the path under it — and no third line: the last response is not a row of
    /// the card, so the height it took is gone with it.
    #[test]
    fn the_card_draws_only_its_subject_and_its_path() {
        let app = AppContext::default();

        let card = |directory: Option<&str>| {
            let mut item = ConversationListItem::new(
                "c1",
                "Ada",
                "40 min ago",
                Rc::new(RefCell::new(AgentCardUi::default())),
                false,
            );
            if let Some(directory) = directory {
                item = item.with_directory(directory);
            }
            let mut element: Box<dyn Element> = Box::new(item);
            let commands =
                crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
            let size = element.size().expect("the card laid out");
            let runs: Vec<String> = commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            (size, runs, commands)
        };

        let (plain, plain_runs, _) = card(None);
        assert_eq!(
            plain_runs,
            ["Ada", "40 min ago"],
            "a card without a directory draws its subject line only"
        );

        let (with_path, runs, commands) = card(Some("~/Projects/goble"));
        assert_eq!(
            runs,
            ["Ada", "40 min ago", "~/Projects/goble"],
            "a card with a directory draws the path under the subject and nothing else"
        );

        let path = run_box(&commands, "~/Projects/goble").expect("the card draws its path");
        assert!(
            (with_path.y - plain.y - (path.height() + 2.0)).abs() < 0.01,
            "the path row and its spacing are the whole height difference: \
             card {with_path:?} plain {plain:?} path {path:?}"
        );
    }

    /// The card keeps content padding of its own: the content starts 12 pt in
    /// from the band's left edge, the way a warp-new list item's does, while the
    /// band — the hover/selected fill and with it the hit area — still spans the
    /// sidebar's full width.
    #[test]
    fn the_cards_content_is_inset_while_its_band_stays_full_width() {
        let app = AppContext::default();
        let width = 260.0;
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "40 min ago",
            Rc::new(RefCell::new(AgentCardUi {
                hover: true,
                menu_open: false,
                ..Default::default()
            })),
            false,
        )
        .with_dots_inset(1.0);
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);
        let card = element.size().expect("the card laid out");
        assert_eq!(card.x, width, "the card is the sidebar's full width");

        // The band: the hover fill covers the whole card, edge to edge.
        let band = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Hover) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the hovered card paints its band");
        assert_eq!(band.min_x(), 0.0, "the band starts at the card's own edge");
        assert_eq!(
            band.max_x(),
            card.x,
            "and spans its full width: {band:?} card {card:?}"
        );
        assert_eq!(
            band.height(),
            card.y,
            "over the card's full height: {band:?} card {card:?}"
        );

        // The content: the harness mark starts at the inset, the subject past
        // the mark's gutter.
        let mark = harness_box(&commands).expect("the card draws its harness mark");
        assert_eq!(
            mark.min_x(),
            CARD_CONTENT_INSET,
            "the mark starts the content's inset in: {mark:?}"
        );
        let title = run_box(&commands, "Ada").expect("the card draws its subject");
        assert_eq!(
            title.min_x(),
            CARD_CONTENT_INSET + HARNESS_SIZE + HARNESS_GAP,
            "the subject starts past the mark's gutter: {title:?}"
        );

        // The dots are placed from the band's trailing edge, so the right side's
        // content inset does not push them a second time.
        let dots = dots_box(&commands).expect("the hovered card draws its dots");
        assert_eq!(
            dots.max_x(),
            card.x - 1.0,
            "the dots keep their own inset from the band's edge: {dots:?}"
        );

        // The selected conversation's band is the same full-width rectangle.
        let selected = ConversationListItem::new(
            "c1",
            "Ada",
            "40 min ago",
            Rc::new(RefCell::new(AgentCardUi::default())),
            true,
        );
        let mut element: Box<dyn Element> = Box::new(selected);
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);
        // 471/remote: a selected row is flat at rest; the surface appears only
        // on hover, matching the Review rows.
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. }
                    if *color == app.theme.color(ColorToken::Hover)
                        || *color == app.theme.color(ColorToken::Selected)
            )),
            "the selected card paints no rest band: {commands:?}"
        );
    }

    /// A conversation that runs on a worker says where it runs right after the
    /// directory it works in: the path belongs to the worker, and the cloud is
    /// what tells a card in the Starred section the same thing the environment
    /// filter told the folders.
    #[test]
    fn a_remote_card_marks_its_directory_with_the_cloud() {
        let app = AppContext::default();
        let render = |routing: &str| {
            let item = ConversationListItem::new(
                "c1",
                "Ada",
                "40 min ago",
                Rc::new(RefCell::new(AgentCardUi::default())),
                false,
            )
            .with_directory("~/workspaces/goble")
            .with_workspace_routing(routing);
            let mut element: Box<dyn Element> = Box::new(item);
            let commands =
                crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
            (
                icon_box(&commands, "cloud"),
                run_box(&commands, "~/workspaces/goble").expect("the card draws its path"),
            )
        };

        let (cloud, path) = render("remote");
        let cloud = cloud.expect("the remote card draws the cloud");
        assert!(
            cloud.min_x() >= path.max_x(),
            "the cloud follows the path: cloud {cloud:?} path {path:?}"
        );
        assert!(
            cloud.min_x() - path.max_x() <= 6.0,
            "and sits right after it: cloud {cloud:?} path {path:?}"
        );
        assert_eq!(cloud.width(), 11.0, "the glyph is the path row's size");
        assert!(
            render("local").0.is_none(),
            "a local card adds nothing: the machine it runs on is this one"
        );
    }

    /// A directory wider than the card is cut at the content's trailing edge,
    /// not at the band's: the inset holds on the right too, so a long path does
    /// not run into the strip the band keeps clear of its own edge.
    #[test]
    fn a_long_directory_is_clipped_at_the_contents_trailing_edge() {
        let app = AppContext::default();
        let width = 260.0;
        let directory = "~/Projects/goble/crates/goble-ui/src/elements/conversation_list_item";
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "40 min ago",
            Rc::new(RefCell::new(AgentCardUi::default())),
            false,
        )
        .with_directory(directory);
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);

        let run = run_box(&commands, directory).expect("the card draws its path");
        assert!(
            run.max_x() > width - CARD_CONTENT_INSET,
            "the path is wider than the content box: {run:?}"
        );

        let narrowest = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::ClipRect(rect) => Some(*rect),
                _ => None,
            })
            .min_by(|a, b| a.max_x().total_cmp(&b.max_x()))
            .expect("the card paints its row clipped");
        assert_eq!(
            narrowest.max_x(),
            width - CARD_CONTENT_INSET,
            "the row is cut at the content's edge, not the band's: {narrowest:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// DSH-parity "Workspaces" sidebar rows
// ---------------------------------------------------------------------------
//
// The 32-point rows the left sidebar draws under its "Workspaces" header. They
// are deliberately separate from `ConversationListItem` (the older, richer
// card): that element and its tests are used by the demo shell and stay
// untouched, while the app sidebar builds these rows instead.
//
// The layout numbers are lifted from DeepSeek Harness 0.2.2-rc.3 and are the
// ones the shared spec (`/tmp/goble-work/sidebar-design-spec.md`) pins down:
// 32pt rows, a card inset 3pt from the row box with a 12pt radius, a 16x20
// leading slot, a 6pt label gap, a 10/16 relative time and a 24x24 action
// button. They are constants (not density-scaled) so the preview scene and the
// tests can assert the same numbers.

use chrono::{DateTime, Utc};

use crate::elements::text::measure_text;
use crate::elements::ConstrainedBox;

/// Height of one session or group row.
pub const SIDEBAR_ROW_HEIGHT: f32 = 32.0;
/// Height of the section header (title + actions).
pub const SIDEBAR_HEADER_HEIGHT: f32 = 36.0;
/// The empty inset the section leaves below the search field / toolbelt.
pub const SIDEBAR_SECTION_TOP_INSET: f32 = 8.0;
/// How far the row's painted card is inset from the row's own box on each side.
pub const SIDEBAR_PAINT_INSET: f32 = 3.0;
/// The rows' corner radius (fixed, not the user's radius setting).
pub const SIDEBAR_ROW_RADIUS: f32 = 12.0;
/// Content inset from the painted card's leading edge.
pub const SIDEBAR_ROW_PADDING_START: f32 = 9.0;
/// Content inset from the painted card's trailing edge.
pub const SIDEBAR_ROW_PADDING_END: f32 = 8.0;
/// Gap between the leading slot and the label.
pub const SIDEBAR_LABEL_GAP: f32 = 6.0;
/// The leading status/folder slot.
pub const SIDEBAR_LEADING_SLOT_W: f32 = 16.0;
pub const SIDEBAR_LEADING_SLOT_H: f32 = 20.0;
/// Gap between the resting trailing cells.
pub const SIDEBAR_TRAILING_GAP: f32 = 6.0;
/// The hover action button's box (the hit target DSH paints the ellipsis in).
pub const SIDEBAR_ACTION_BUTTON: f32 = 24.0;
/// The layout footprint of a hover-strip control: DSH's 24 pt button carries a
/// -4 pt margin, so it occupies 16 pt in the row and the 32 pt row never grows.
/// At rest a session row holds just the dots' cell; while its strip is shown the
/// title cell hands the strip's reserved room over (DSH's flex row does the
/// same when `rowResting` drops for `rowActions`).
pub const SIDEBAR_ACTION_SLOT: f32 = 16.0;
/// The hover strip's button: DSH's 24pt iconButton carries a -4pt margin, so
/// each control occupies 16pt of the row and its 24pt hit box spills into the
/// 13pt gap beside it (24 < 16 + 13, so neighbouring targets never overlap).
pub const SIDEBAR_QUICK_SLOT: f32 = SIDEBAR_ACTION_SLOT;
/// Gap between the hover strip's controls (DSH `rowActions` gap: 13px).
pub const SIDEBAR_QUICK_GAP: f32 = 13.0;
/// The hit target of one hover-strip control (DSH's 24pt iconButton).
pub const SIDEBAR_QUICK_HIT: f32 = SIDEBAR_ACTION_BUTTON;
/// The quick actions' glyph size: DSH's Archive/Pin/NewSession buttons draw
/// their icons at 14 (the ellipsis trigger keeps the 16pt default).
pub const SIDEBAR_QUICK_ICON: f32 = 14.0;
/// The hover strip's width for `buttons` controls, right-aligned in the row.
pub fn sidebar_quick_strip_width(buttons: usize) -> f32 {
    let n = buttons.max(1) as f32;
    n * SIDEBAR_QUICK_SLOT + (n - 1.0) * SIDEBAR_QUICK_GAP
}
/// One session row's hover strip: More, then Archive (or Unarchive), then the
/// pin toggle. An archived row offers nothing to pin, and its archive toggle is
/// the restore, so it carries two controls instead of three (DSH resolves the
/// same list: `PinSessionRowButton` returns null on an archived row).
pub fn session_quick_buttons(archived: bool) -> usize {
    if archived {
        2
    } else {
        3
    }
}
/// One Workspace row's hover strip: its context-menu trigger (More) and the
/// New-session button (DSH's `ProjectRow` `rowActions`: the menu trigger, then
/// the create control).
pub const WORKSPACE_QUICK_BUTTONS: usize = 2;

/// The open-menu registry's id for a session row's tray. The window keeps one
/// menu at a time (see [`crate::elements::OpenMenus`]), and the rows claim,
/// report and close their trays through it like every other menu.
pub const SESSION_TRAY_MENU_ID: MenuId = "sidebar.session_row";
/// The open-menu registry's id for a workspace row's tray.
pub const WORKSPACE_TRAY_MENU_ID: MenuId = "sidebar.workspace_row";
/// The section header's icon buttons.
pub const SIDEBAR_HEADER_BUTTON: f32 = 28.0;
/// The section header's glyphs.
pub const SIDEBAR_HEADER_ICON: f32 = 16.0;
/// The section header's title inset from the painted card's edge.
pub const SIDEBAR_HEADER_PADDING_START: f32 = 12.0;
/// Gap between rows inside one group.
pub const SIDEBAR_ROW_GAP: f32 = 2.0;
/// Extra gap between two groups.
pub const SIDEBAR_GROUP_GAP: f32 = 4.0;
/// The "View all"/"Show less" button.
pub const SIDEBAR_OVERFLOW_HEIGHT: f32 = 28.0;
/// The overflow button's label indent (12 + 16 + 6).
pub const SIDEBAR_OVERFLOW_INDENT: f32 = 34.0;
/// Bottom padding under the last row.
pub const SIDEBAR_LIST_BOTTOM_PAD: f32 = 16.0;
/// The session title's font size and line height.
pub const SIDEBAR_TITLE_SIZE: f32 = 14.0;
pub const SIDEBAR_TITLE_LINE: f32 = 20.0;
/// The relative time's font size and line height.
pub const SIDEBAR_TIME_SIZE: f32 = 10.0;
pub const SIDEBAR_TIME_LINE: f32 = 16.0;
/// The harness mark in the resting trailing cluster.
pub const SIDEBAR_HARNESS_MARK: f32 = 12.0;
/// The pin glyph in the resting trailing cluster.
pub const SIDEBAR_PIN_SIZE: f32 = 14.0;
/// The status dot's box and core.
pub const SIDEBAR_STATUS_DOT: f32 = 10.0;
/// The running spinner ring's size.
pub const SIDEBAR_RUNNING_RING: f32 = 14.0;
/// The section header's title copy.
pub const SIDEBAR_SECTION_TITLE: &str = "Projects";

/// Atlas glyphs the Workspaces rows draw.
pub const SIDEBAR_ICON_FOLDER_CLOSE: &str = "folder-close";
pub const SIDEBAR_ICON_FOLDER_OPEN: &str = "folder-open";
pub const SIDEBAR_ICON_TRIANGLE_RIGHT: &str = "triangle-right-fill";
pub const SIDEBAR_ICON_TRIANGLE_DOWN: &str = "triangle-down-fill";
/// The row's context-menu trigger; Goble already ships the same three dots.
pub const SIDEBAR_ICON_ELLIPSIS: &str = "dots-horizontal";
pub const SIDEBAR_ICON_PIN_FILL: &str = "pin-fill";
pub const SIDEBAR_ICON_PIN_OUTLINE: &str = "pin-outline";
/// The hover strip's Archive control, and its restore face on an archived row.
pub const SIDEBAR_ICON_ARCHIVE: &str = "archive";
pub const SIDEBAR_ICON_UNARCHIVE: &str = "unarchive";
pub const SIDEBAR_ICON_STATUS_RUNNING: &str = "status-running";
pub const SIDEBAR_ICON_SLIDERS: &str = "sliders-two";
pub const SIDEBAR_ICON_PROJECT_ADD: &str = "project-add";
pub const SIDEBAR_ICON_NEW_CHAT: &str = "new-chat";
pub const SIDEBAR_ICON_CLOUD: &str = "cloud";

/// A compact, DSH-style relative time: `now`, `4min`, `17h`, `22h`, `2d`,
/// `3mo`, `1y`. Falls back to the raw string when it cannot be read.
pub fn sidebar_relative_time(updated_at: &str) -> String {
    sidebar_relative_time_at(Utc::now(), updated_at)
}

/// [`sidebar_relative_time`] against an explicit clock, for tests.
pub fn sidebar_relative_time_at(now: DateTime<Utc>, updated_at: &str) -> String {
    let Ok(ts) = DateTime::parse_from_rfc3339(updated_at.trim()) else {
        return updated_at.to_string();
    };
    let secs = now
        .signed_duration_since(ts.with_timezone(&Utc))
        .num_seconds()
        .max(0);
    if secs < 60 {
        "now".to_string()
    } else if secs < 3_600 {
        format!("{}min", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else if secs < 2_592_000 {
        format!("{}d", secs / 86_400)
    } else if secs < 31_536_000 {
        format!("{}mo", secs / 2_592_000)
    } else {
        format!("{}y", secs / 31_536_000)
    }
}

/// The hover card's relative time, worded like the web's: `just now`, `5m ago`,
/// `1h ago`, `3d ago`, and a date for older rows.
pub fn sidebar_relative_time_long(updated_at: &str) -> String {
    sidebar_relative_time_long_at(Utc::now(), updated_at)
}

/// [`sidebar_relative_time_long`] against an explicit clock, for tests.
pub fn sidebar_relative_time_long_at(now: DateTime<Utc>, updated_at: &str) -> String {
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(updated_at.trim()) else {
        return updated_at.to_string();
    };
    let secs = now
        .signed_duration_since(ts.with_timezone(&Utc))
        .num_seconds()
        .max(0);
    if secs < 45 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 7 * 86_400 {
        format!("{}d ago", secs / 86_400)
    } else {
        ts.format("%b %-d").to_string()
    }
}

/// Cut `text` to `max_width`, ending in an ellipsis when anything had to go.
/// `Text` only hard-clips, so the sidebar rows measure and truncate the title
/// in the builder — the spec's recommendation for the sidebar only.
pub fn truncate_with_ellipsis(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if !max_width.is_finite() {
        return text.to_string();
    }
    let natural = measure_text(text, font_size, line_height, f32::INFINITY).x;
    if natural <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_width = measure_text(ellipsis, font_size, line_height, f32::INFINITY).x;
    if max_width <= ellipsis_width {
        return ellipsis.to_string();
    }
    let mut kept = String::new();
    for ch in text.chars() {
        let mut candidate = kept.clone();
        candidate.push(ch);
        let width =
            measure_text(&candidate, font_size, line_height, f32::INFINITY).x + ellipsis_width;
        if width > max_width {
            break;
        }
        kept = candidate;
    }
    kept.push('…');
    kept
}

/// The row a drag started on, and where it was pressed.
#[derive(Clone, Debug)]
pub struct SidebarDragPress {
    pub id: String,
    pub group: String,
    pub at: Vector2F,
}

/// The app-owned drag state of the sidebar list. The tree is rebuilt every
/// frame, so the press/active/marker state lives here (shared through
/// `Rc<RefCell<_>>`), exactly like the editor's other per-frame state.
#[derive(Clone, Debug, Default)]
pub struct SidebarDrag {
    /// The row the pointer went down on, before it travelled far enough to
    /// count as a drag.
    pub press: Option<SidebarDragPress>,
    /// Whether the press has become a drag (the pointer moved past the slop).
    pub active: bool,
    /// The row the insert marker sits on and whether it is below that row.
    pub marker: Option<(String, bool)>,
}

impl SidebarDrag {
    /// Forget the whole gesture.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Whether `id` currently carries the insert marker, and on which side.
    pub fn marker_for(&self, id: &str) -> Option<bool> {
        self.marker
            .as_ref()
            .filter(|(target, _)| target == id)
            .map(|(_, after)| *after)
    }
}

/// A glyph boxed into the 16x20 leading slot.
fn sidebar_leading_slot(glyph: Box<dyn Element>, size: f32) -> Box<dyn Element> {
    let horizontal = ((SIDEBAR_LEADING_SLOT_W - size) / 2.0).max(0.0);
    let vertical = ((SIDEBAR_LEADING_SLOT_H - size) / 2.0).max(0.0);
    Container::new(glyph)
        .with_padding(EdgeInsets::new(horizontal, vertical, horizontal, vertical))
        .finish()
}

/// The 10pt status dot DSH draws for warning/error states: a 6pt core in a
/// 10pt box.
fn sidebar_status_dot(color: ColorToken, app: &AppContext) -> Box<dyn Element> {
    let core = SIDEBAR_STATUS_DOT * 0.6;
    let inset = (SIDEBAR_STATUS_DOT - core) / 2.0;
    Container::new(
        Container::new(Empty::new().with_size(vec2f(core, core)).finish())
            .with_background(Fill::Solid(app.theme.color(color)))
            .with_corner_radius(core / 2.0)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(inset))
    .finish()
}

/// Render one trailing resting cell.
fn sidebar_time_text(text: String, app: &AppContext) -> Box<dyn Element> {
    Text::new(text)
        .with_font_size(SIDEBAR_TIME_SIZE)
        .with_line_height(SIDEBAR_TIME_LINE / SIDEBAR_TIME_SIZE)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

/// The width the resting trailing cluster takes for a measured time label.
fn resting_cluster_width(time_label: &str, remote: bool, starred: bool) -> f32 {
    let time = measure_text(
        time_label,
        SIDEBAR_TIME_SIZE,
        SIDEBAR_TIME_LINE,
        f32::INFINITY,
    )
    .x;
    let mut width = SIDEBAR_HARNESS_MARK + SIDEBAR_TRAILING_GAP + time;
    if remote {
        width += SIDEBAR_TRAILING_GAP + SIDEBAR_HARNESS_MARK;
    }
    if starred {
        width += SIDEBAR_TRAILING_GAP + SIDEBAR_PIN_SIZE;
    }
    width
}

/// The session row's trailing cell width: the row's own resting cluster width,
/// floored at the action button's box. The same value is used in **every**
/// state — rest, hover and tray-open — so the title cell keeps one width and
/// the baked ellipsis never reflows (and the row never changes height). The
/// resting state renders today's cluster plus a spacer up to this slot, so the
/// rest layout stays pixel-identical to before.
fn trailing_slot_width(time_label: &str, remote: bool, starred: bool) -> f32 {
    resting_cluster_width(time_label, remote, starred).max(SIDEBAR_ACTION_BUTTON)
}

/// One 28x28 icon button of the section header (or the sidebar toolbelt).
///
/// Hover is app-owned (the tree is rebuilt every frame): the caller passes the
/// shared cell and this row's key, reads the cell while building the next frame
/// to pick ink and fill, and the element writes its own hover into the cell at
/// paint time, exactly like `HoverRow::with_hover_key`.
pub struct SidebarIconButton {
    icon: &'static str,
    radius: Option<f32>,
    button_size: f32,
    icon_size: f32,
    hovered: bool,
    /// Whether this control is the selected one (the toolbelt's current view).
    active: bool,
    hover_key: Option<(Rc<RefCell<Option<String>>>, String)>,
    on_click: Option<Rc<RefCell<dyn FnMut()>>>,
    child: Option<Box<dyn Element>>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarIconButton {
    pub fn new(icon: &'static str) -> Self {
        Self {
            icon,
            radius: None,
            button_size: SIDEBAR_HEADER_BUTTON,
            icon_size: SIDEBAR_HEADER_ICON,
            hovered: false,
            active: false,
            hover_key: None,
            on_click: None,
            child: None,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    /// Draw the control as the selected one (the toolbelt's current view).
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// The button's box; the header's are 28x28.
    pub fn with_button_size(mut self, size: f32) -> Self {
        self.button_size = size;
        self
    }

    /// The glyph's size inside the box.
    pub fn with_icon_size(mut self, size: f32) -> Self {
        self.icon_size = size;
        self
    }

    /// Round the hover fill; the header uses the theme's radius, the row's
    /// action button the spec's 4.
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Whether this control was hovered on the last frame.
    pub fn with_hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Report hover into the shared cell under `key`.
    pub fn with_hover_cell(
        mut self,
        cell: Rc<RefCell<Option<String>>>,
        key: impl Into<String>,
    ) -> Self {
        self.hover_key = Some((cell, key.into()));
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.child.is_some() {
            return;
        }
        let ink = if self.hovered || self.active {
            ColorToken::Text
        } else {
            ColorToken::Muted
        };
        let glyph = Icon::new(self.icon)
            .with_size(self.icon_size)
            .with_theme_color(ink, app)
            .finish();
        let inset = (self.button_size - self.icon_size) / 2.0;
        self.child = Some(
            Container::new(glyph)
                .with_padding(EdgeInsets::uniform(inset.max(0.0)))
                .finish(),
        );
    }
}

impl Element for SidebarIconButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let tight = SizeConstraint::tight(vec2f(self.button_size, self.button_size));
        let _ = self.child.as_mut().unwrap().layout(tight, ctx, app);
        let _ = constraint;
        self.size = Some(vec2f(self.button_size, self.button_size));
        vec2f(self.button_size, self.button_size)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        if let Some((cell, key)) = &self.hover_key {
            let hovered = ctx.hovered(bounds);
            let mut cell = cell.borrow_mut();
            if hovered {
                *cell = Some(key.clone());
            } else if cell.as_deref() == Some(key.as_str()) {
                *cell = None;
            }
        }
        if self.hovered {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(
                    bounds,
                    app.theme.color(ColorToken::Hover),
                    self.radius.unwrap_or_else(|| app.theme.radius_px()),
                );
            }
        }
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let _ = app;
        let Some(bounds) = self.bounds() else {
            return false;
        };
        let on_click = self.on_click.clone();
        let mut run = move || {
            if let Some(cb) = on_click.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut run)
    }
}

impl SidebarIconButton {
    /// Box the control for a parent's child list.
    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

/// One workspace group row's open-tray state: whether its menu is up and which
/// row the keyboard highlight is on. The app keeps one of these per group so the
/// tray survives the per-frame element rebuild (the session rows' [`AgentCardUi`]
/// is the same contract).
#[derive(Clone, Debug, Default)]
pub struct SidebarGroupUi {
    /// Whether the row's workspace tray is open.
    pub menu_open: bool,
    /// The tray row the keyboard highlight is on.
    pub highlight: usize,
}

impl SidebarGroupUi {
    /// Put the tray away and reset the keyboard highlight.
    pub fn close_menu(&mut self) {
        self.menu_open = false;
        self.highlight = NO_HIGHLIGHT;
    }
}

/// One workspace row: the folder (or the disclosure triangle while hovered) and
/// the workspace's label. Clicking it toggles the group.
///
/// The hover strip is DSH's `ProjectRow` `rowActions`, in its order: the
/// context-menu trigger (the three dots) and the New-session button (the
/// `new-chat` glyph). Both sit in 16pt slots with a 13pt gap and a 24pt hit
/// target, at the row's trailing edge; the label cell keeps clear of the strip
/// while it is shown, so the row never grows past its 32pt band. The tray the
/// dots open carries the workspace's own actions (Rename / Show in Finder /
/// Delete workspace), supplied by the app exactly like the session row's tray
/// rows.
pub struct SidebarGroupRow {
    label: String,
    /// A fixed leading glyph (the Starred group's pin). `None` draws the
    /// folder, swapped for the disclosure triangle while hovered.
    leading_icon: Option<&'static str>,
    /// A plain heading with no leading glyph at all (the label starts where
    /// the icon slot used to be).
    hide_leading_icon: bool,
    expanded: bool,
    /// Whether the group holds the selected conversation: its folder takes the
    /// theme accent, the way DSH blues the folder of the active workspace.
    active: bool,
    hovered: bool,
    /// The workspace tray's rows, supplied by the app (labels, icons,
    /// separators, actions). Empty means no dots control is drawn.
    menu_items: Vec<SidebarMenuItem>,
    /// The tray's open/highlight state, kept by the app across rebuilds.
    ui: Rc<RefCell<SidebarGroupUi>>,
    /// The window's height, for the tray's flip-above clamp (0 = unknown).
    viewport_height: f32,
    hover_key: Option<(Rc<RefCell<Option<String>>>, String)>,
    on_click: Option<Rc<RefCell<dyn FnMut()>>>,
    /// The New-session control: start a conversation inside this workspace.
    on_create_session: Option<Rc<RefCell<dyn FnMut()>>>,
    /// Called when the strip opens the tray, so the app can close every other
    /// menu it owns (one menu is open at a time).
    on_menu_open: Option<Rc<RefCell<dyn FnMut()>>>,
    child: Option<Box<dyn Element>>,
    tray: Option<Rc<RefCell<Box<dyn Element>>>>,
    tray_size: Option<Vector2F>,
    label_width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
    state: InteractiveState,
}

/// A session row's hover-strip controls, left to right (DSH's `rowActions`:
/// the context-menu trigger, then the registered quick actions — Archive, then
/// Pin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionQuick {
    /// The three-dot context-menu trigger.
    More,
    /// Archive the conversation (or restore an archived one).
    Archive,
    /// Toggle the conversation's pin.
    Pin,
}

/// The workspace row's hover-strip controls, left to right (DSH's
/// `ProjectRow`: the context-menu trigger, then the create control).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspaceQuick {
    /// The three-dot context-menu trigger.
    More,
    /// Start a conversation in this workspace.
    NewSession,
}

impl SidebarGroupRow {
    pub fn new(label: impl Into<String>, expanded: bool) -> Self {
        Self {
            label: label.into(),
            leading_icon: None,
            hide_leading_icon: false,
            expanded,
            active: false,
            hovered: false,
            menu_items: Vec::new(),
            ui: Rc::new(RefCell::new(SidebarGroupUi::default())),
            viewport_height: 0.0,
            hover_key: None,
            on_click: None,
            on_create_session: None,
            on_menu_open: None,
            child: None,
            tray: None,
            tray_size: None,
            label_width: 0.0,
            size: None,
            origin: None,
            state: InteractiveState::default(),
        }
    }

    /// Lead with a fixed glyph instead of the folding folder (the Starred
    /// group's pin).
    pub fn with_leading_icon(mut self, icon: &'static str) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Draw the heading with no leading glyph; the label takes the icon slot's
    /// width, so it starts where the folder used to be.
    pub fn without_leading_icon(mut self) -> Self {
        self.hide_leading_icon = true;
        self
    }

    /// Mark the group that holds the selected conversation.
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Whether the row was hovered on the last frame (folder → triangle swap
    /// and the action strip's appearance).
    pub fn with_hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    pub fn with_hover_cell(
        mut self,
        cell: Rc<RefCell<Option<String>>>,
        key: impl Into<String>,
    ) -> Self {
        self.hover_key = Some((cell, key.into()));
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The tray rows, in order. The app owns every label and whether a row is
    /// offered (a folder that is only implied by its sessions has no stored
    /// workspace to delete, for example).
    pub fn with_menu_items(mut self, items: Vec<SidebarMenuItem>) -> Self {
        self.menu_items = items;
        self
    }

    /// The app-owned open/highlight state for this row's tray.
    pub fn with_ui(mut self, ui: Rc<RefCell<SidebarGroupUi>>) -> Self {
        self.ui = ui;
        self
    }

    /// The window's height, so a tray opened near the bottom edge flips above
    /// its row instead of running under the window.
    pub fn with_viewport_height(mut self, height: f32) -> Self {
        self.viewport_height = height;
        self
    }

    /// The New-session control: start a conversation inside this workspace.
    pub fn with_on_create_session<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_create_session = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Called when the dots open the tray (never while it closes), so the app
    /// can close its other menus.
    pub fn with_on_menu_open<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_menu_open = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The glyph the row leads with on this frame.
    fn glyph(&self) -> (&'static str, f32, ColorToken) {
        if let Some(icon) = self.leading_icon {
            return (
                icon,
                SIDEBAR_HEADER_ICON,
                if self.active {
                    ColorToken::Focus
                } else {
                    ColorToken::Muted
                },
            );
        }
        if self.hovered {
            (
                if self.expanded {
                    SIDEBAR_ICON_TRIANGLE_DOWN
                } else {
                    SIDEBAR_ICON_TRIANGLE_RIGHT
                },
                SIDEBAR_PIN_SIZE,
                ColorToken::Muted,
            )
        } else {
            (
                if self.expanded {
                    SIDEBAR_ICON_FOLDER_OPEN
                } else {
                    SIDEBAR_ICON_FOLDER_CLOSE
                },
                SIDEBAR_HEADER_ICON,
                if self.active {
                    ColorToken::Focus
                } else {
                    ColorToken::Muted
                },
            )
        }
    }

    /// Whether the tray is up (it stays up while the pointer leaves the row).
    fn menu_open(&self) -> bool {
        self.ui.borrow().menu_open
    }

    /// Whether the row is showing its action strip: hover, or the tray it keeps
    /// open after the pointer walked onto the tray (DSH's `menuOpen`).
    fn showing_actions(&self) -> bool {
        self.hovered || self.menu_open()
    }

    /// The hover strip's width for this row (0 when it is not shown).
    fn strip_width(&self) -> f32 {
        if self.showing_actions() && !self.menu_items.is_empty() {
            sidebar_quick_strip_width(WORKSPACE_QUICK_BUTTONS)
        } else {
            0.0
        }
    }

    /// The glyph box and the hit box of each strip control, left to right. The
    /// glyphs sit inside the strip's reserved width; the 24pt hit boxes are
    /// centred on them, spilling 4pt into the gaps (never into a neighbour).
    fn quick_action_boxes(&self) -> Vec<(WorkspaceQuick, RectF, RectF)> {
        let (Some(origin), Some(size)) = (self.origin, self.size) else {
            return Vec::new();
        };
        if !self.showing_actions() {
            return Vec::new();
        }
        let count = WORKSPACE_QUICK_BUTTONS;
        let right = origin.x() + size.x - SIDEBAR_PAINT_INSET - SIDEBAR_ROW_PADDING_END;
        let kinds = [WorkspaceQuick::More, WorkspaceQuick::NewSession];
        let mut boxes = Vec::with_capacity(count);
        for (index, kind) in kinds.into_iter().enumerate() {
            let from_right = (count - 1 - index) as f32;
            let glyph_x =
                right - SIDEBAR_QUICK_SLOT - from_right * (SIDEBAR_QUICK_SLOT + SIDEBAR_QUICK_GAP);
            let glyph = rectf(
                glyph_x,
                origin.y() + (size.y - SIDEBAR_LEADING_SLOT_H) / 2.0,
                SIDEBAR_QUICK_SLOT,
                SIDEBAR_LEADING_SLOT_H,
            );
            let hit = rectf(
                glyph_x + (SIDEBAR_QUICK_SLOT - SIDEBAR_QUICK_HIT) / 2.0,
                origin.y() + (size.y - SIDEBAR_QUICK_HIT) / 2.0,
                SIDEBAR_QUICK_HIT,
                SIDEBAR_QUICK_HIT,
            );
            boxes.push((kind, glyph, hit));
        }
        boxes
    }

    /// The actionable tray row a highlight index lands on (separators are flags
    /// on rows, not rows). [`NO_HIGHLIGHT`] lands on the first row: Enter on a
    /// freshly opened tray runs its first row, as DSH's focused first item does.
    fn menu_row(&self, index: usize) -> usize {
        let count = self.menu_items.len();
        if count == 0 || index >= count {
            0
        } else {
            index
        }
    }

    fn tray_bounds(&self) -> Option<RectF> {
        let (origin, size) = (self.origin?, self.size?);
        let tray_size = self.tray_size?;
        self.tray.as_ref()?;
        let below = origin.y() + size.y + TRAY_GAP;
        let fits_below =
            self.viewport_height <= 0.0 || below + tray_size.y <= self.viewport_height - 4.0;
        let y = if fits_below {
            below
        } else {
            (origin.y() - TRAY_GAP - tray_size.y).max(4.0)
        };
        let x = (origin.x() + size.x - SIDEBAR_PAINT_INSET - tray_size.x).max(SIDEBAR_PAINT_INSET);
        Some(rectf(x, y, tray_size.x, tray_size.y))
    }

    /// The 4pt corridor between the row and its tray: part of the tray's hit
    /// area (and of the open-menu surface reported to the root view), so the
    /// pointer can walk from the dots onto the tray without leaving the menu.
    fn tray_corridor(&self) -> Option<RectF> {
        let bounds = self.tray_bounds()?;
        let row_bottom = match (self.origin, self.size) {
            (Some(origin), Some(size)) => origin.y() + size.y,
            _ => bounds.min_y(),
        };
        let (bridge_y, bridge_h) = if bounds.min_y() >= row_bottom {
            (bounds.min_y() - TRAY_GAP, TRAY_GAP)
        } else {
            (bounds.max_y(), TRAY_GAP)
        };
        Some(rectf(bounds.min_x(), bridge_y, bounds.width(), bridge_h))
    }

    /// The close handle the window's open-menu registry runs: it is the tray's
    /// own close, so Escape, an outside press and another row opening its menu
    /// all land here.
    fn tray_close(&self) -> MenuClose {
        let ui = Rc::clone(&self.ui);
        Rc::new(RefCell::new(move || ui.borrow_mut().close_menu()))
    }

    /// The dots' hit box, the registry's anchor for the tray.
    fn tray_anchor(&self) -> Option<RectF> {
        self.quick_action_boxes()
            .into_iter()
            .find(|(kind, _, _)| *kind == WorkspaceQuick::More)
            .map(|(_, _, hit)| hit)
    }

    fn pointer_on_tray(&self, position: Vector2F) -> bool {
        let Some(bounds) = self.tray_bounds() else {
            return false;
        };
        contains(bounds, position)
            || self
                .tray_corridor()
                .is_some_and(|corridor| contains(corridor, position))
    }

    fn menu_takes_event(&self, event: &DispatchedEvent) -> bool {
        match pointer_position(event) {
            None => true,
            Some(position) => self.pointer_on_tray(position),
        }
    }

    /// The tray's keys, answered only while it is open.
    fn menu_key(&mut self, event: &DispatchedEvent) -> bool {
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if modifiers.command || modifiers.ctrl || modifiers.alt || !self.menu_open() {
            return false;
        }
        let count = self.menu_items.len();
        if count == 0 {
            return false;
        }
        match key.as_str() {
            "ArrowUp" | "ArrowDown" => {
                let stored = self.ui.borrow().highlight;
                let next = match (key.as_str(), stored >= count) {
                    // Nothing is highlighted yet: Down enters at the first row,
                    // Up enters at the last (DSH's menu walk).
                    ("ArrowDown", true) => 0,
                    ("ArrowUp", true) => count - 1,
                    ("ArrowDown", false) => (stored + 1).min(count - 1),
                    (_, false) => stored.saturating_sub(1),
                    _ => unreachable!(),
                };
                self.ui.borrow_mut().highlight = next;
                true
            }
            "Enter" => {
                let highlighted = self.menu_row(self.ui.borrow().highlight);
                self.run_menu_row(highlighted);
                true
            }
            "Escape" => {
                self.ui.borrow_mut().close_menu();
                true
            }
            _ => false,
        }
    }

    fn run_menu_row(&self, index: usize) {
        self.ui.borrow_mut().close_menu();
        if let Some(item) = self.menu_items.get(index) {
            (item.run.borrow_mut())();
        }
    }

    /// Open or close the tray. Opening claims the window's one open menu (which
    /// closes whatever was open) and tells the app, so every other menu it owns
    /// goes away first (DSH's context-menu contract).
    fn toggle_menu(&mut self, app: &AppContext) {
        let open = self.menu_open();
        if open {
            // Through the registry, so the dismissal path is the same one
            // Escape and an outside press take.
            app.open_menus.close();
            return;
        }
        if self.menu_items.is_empty() {
            return;
        }
        app.open_menus
            .open(WORKSPACE_TRAY_MENU_ID, self.tray_close());
        if let Some(callback) = self.on_menu_open.clone() {
            (callback.borrow_mut())();
        }
        let mut ui = self.ui.borrow_mut();
        if !ui.menu_open {
            ui.menu_open = true;
            ui.highlight = NO_HIGHLIGHT;
        }
    }

    fn ensure_root(&mut self, app: &AppContext, label_width: Option<f32>) {
        if self.child.is_some() {
            return;
        }
        let menu_open = self.menu_open();
        let highlight = self.ui.borrow().highlight;
        let (glyph, size, color) = self.glyph();
        let leading = (!self.hide_leading_icon).then(|| {
            sidebar_leading_slot(
                Icon::new(glyph)
                    .with_size(size)
                    .with_theme_color(color, app)
                    .finish(),
                size,
            )
        });
        let label = match label_width {
            Some(width) if width.is_finite() => {
                truncate_with_ellipsis(&self.label, SIDEBAR_TITLE_SIZE, SIDEBAR_TITLE_LINE, width)
            }
            _ => self.label.clone(),
        };
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(SIDEBAR_LABEL_GAP);
        if let Some(leading) = leading {
            row = row.with_child(leading);
        }
        let mut row = row.with_child(
            Expanded::new(
                Padding::new(
                    Text::new(label)
                        .with_font_size(SIDEBAR_TITLE_SIZE)
                        .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
                        .with_max_lines(1)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                    EdgeInsets::new(0.0, 0.0, 0.0, 0.0),
                )
                .finish(),
            )
            .finish(),
        );
        // The strip's layout footprint: the label keeps clear of it while the
        // controls are shown, exactly as DSH's flex row does.
        let strip = self.strip_width();
        if strip > 0.0 {
            row = row.with_child(
                Empty::new()
                    .with_size(vec2f(strip, SIDEBAR_LEADING_SLOT_H))
                    .finish(),
            );
        }
        self.label_width = label_width.unwrap_or(0.0);
        self.child = Some(
            Container::new(row.finish())
                .with_padding(EdgeInsets::new(
                    SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_START,
                    (SIDEBAR_ROW_HEIGHT - SIDEBAR_LEADING_SLOT_H) / 2.0,
                    SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_END,
                    (SIDEBAR_ROW_HEIGHT - SIDEBAR_LEADING_SLOT_H) / 2.0,
                ))
                .finish(),
        );
        if menu_open {
            let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, item) in self.menu_items.iter().enumerate() {
                if index > 0 && item.separator_before {
                    rows = rows.with_child(Box::new(SidebarTraySeparator {
                        size: None,
                        origin: None,
                    }));
                }
                let ui = Rc::clone(&self.ui);
                let run = Rc::clone(&item.run);
                rows =
                    rows.with_child(sidebar_menu_row(app, item, highlight == index, move || {
                        ui.borrow_mut().close_menu();
                        (run.borrow_mut())();
                    }));
            }
            let tray = sidebar_menu_tray(app, rows.finish(), &self.menu_items);
            self.tray = Some(Rc::new(RefCell::new(tray)));
        }
    }
}

impl Element for SidebarGroupRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The label cell the strip leaves: the content width less the leading
        // slot, both gaps and the strip. A row that is not showing its strip
        // gives the label the whole run, like DSH's flex row.
        let strip = self.strip_width();
        let trailing = if strip > 0.0 {
            strip + SIDEBAR_LABEL_GAP
        } else {
            0.0
        };
        let label_width = if constraint.max.x.is_finite() {
            Some(
                (constraint.max.x
                    - 2.0 * SIDEBAR_PAINT_INSET
                    - SIDEBAR_ROW_PADDING_START
                    - SIDEBAR_ROW_PADDING_END
                    - if false {
                        0.0
                    } else {
                        SIDEBAR_LEADING_SLOT_W + SIDEBAR_LABEL_GAP
                    }
                    - trailing)
                    .max(0.0),
            )
        } else {
            None
        };
        self.ensure_root(app, label_width);
        let size = self.child.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        self.tray_size = None;
        if let Some(tray) = self.tray.as_ref() {
            let tray_size = tray.borrow_mut().layout(
                SizeConstraint::loose(vec2f(SIDEBAR_MENU_MAX_WIDTH, 400.0)),
                ctx,
                app,
            );
            self.tray_size = Some(tray_size);
        }
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        if let Some((cell, key)) = &self.hover_key {
            let hovered = ctx.hovered(bounds);
            let mut cell = cell.borrow_mut();
            if hovered {
                *cell = Some(key.clone());
            } else if cell.as_deref() == Some(key.as_str()) {
                *cell = None;
            }
        }
        let menu_open = self.menu_open();
        let card = rectf(
            origin.x + SIDEBAR_PAINT_INSET,
            origin.y,
            (size.x - 2.0 * SIDEBAR_PAINT_INSET).max(0.0),
            size.y,
        );
        if let Some(renderer) = ctx.renderer.as_mut() {
            let fill = if menu_open {
                Some(ColorToken::Selected)
            } else if self.hovered {
                Some(ColorToken::Hover)
            } else {
                None
            };
            if let Some(token) = fill {
                renderer.fill_rounded_rect(card, app.theme.color(token), SIDEBAR_ROW_RADIUS);
            }
            renderer.clip_rect(card);
        }
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }
        // The strip's glyphs, painted after the label so they sit above it and
        // read muted at rest, primary while the pointer is on the control.
        for (kind, glyph, hit) in self.quick_action_boxes() {
            let icon = match kind {
                WorkspaceQuick::More => SIDEBAR_ICON_ELLIPSIS,
                WorkspaceQuick::NewSession => SIDEBAR_ICON_NEW_CHAT,
            };
            let icon_size = match kind {
                WorkspaceQuick::More => SIDEBAR_ACTION_SLOT,
                WorkspaceQuick::NewSession => SIDEBAR_QUICK_ICON,
            };
            let ink = if ctx.hovered(hit) {
                ColorToken::Text
            } else {
                ColorToken::Muted
            };
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.draw_icon(
                    vec2f(
                        glyph.min_x() + (SIDEBAR_QUICK_SLOT - icon_size) / 2.0,
                        glyph.min_y() + (SIDEBAR_LEADING_SLOT_H - icon_size) / 2.0,
                    ),
                    icon,
                    icon_size,
                    app.theme.color(ink),
                );
            }
        }
        // The tray reports itself and its surface to the window's open-menu
        // registry and paints inside a menu surface, so its own hover survives
        // while the rest of the window is gated.
        if let Some(tray_bounds) = self.tray_bounds() {
            if let Some(tray) = self.tray.as_ref() {
                let at = vec2f(tray_bounds.min_x(), tray_bounds.min_y());
                let mut surfaces = vec![tray_bounds];
                surfaces.extend(self.tray_corridor());
                app.open_menus.report(
                    WORKSPACE_TRAY_MENU_ID,
                    self.tray_close(),
                    surfaces,
                    MenuPointer::Paint,
                );
                if let Some(anchor) = self.tray_anchor() {
                    app.open_menus.report_anchor(WORKSPACE_TRAY_MENU_ID, anchor);
                }
                ctx.enter_menu_surface();
                tray.borrow_mut().paint(at, ctx, app);
                app.hover_chips
                    .borrow_mut()
                    .push_shared(at, Rc::clone(tray));
                ctx.leave_menu_surface();
            }
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if self.child.is_none() {
            self.ensure_root(app, None);
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };

        if self.menu_key(event) {
            return true;
        }

        // The strip's own controls take the press before the row does: the
        // dots toggle the tray, the create control starts a conversation. Only
        // the press acts (the release is consumed with it) so the row's own
        // toggle never fires from the same click.
        if self.showing_actions() {
            if let Some(position) = pointer_position(event) {
                for (kind, _, hit) in self.quick_action_boxes() {
                    if !contains(hit, position) {
                        continue;
                    }
                    match event {
                        DispatchedEvent::MouseDown { .. } => {
                            match kind {
                                WorkspaceQuick::More => self.toggle_menu(app),
                                WorkspaceQuick::NewSession => {
                                    if let Some(callback) = self.on_create_session.clone() {
                                        (callback.borrow_mut())();
                                    }
                                }
                            }
                            return true;
                        }
                        DispatchedEvent::MouseUp { .. } => return true,
                        _ => {}
                    }
                }
            }
        }

        if self.menu_open() {
            let takes_menu = self.menu_takes_event(event);
            if takes_menu {
                if let Some(tray) = self.tray.as_mut() {
                    if tray.borrow_mut().dispatch_event(event, ctx, app) {
                        return true;
                    }
                }
            }
            // A press on the row itself (outside the tray) dismisses the tray
            // first: the row underneath is a toggle, and running it while the
            // menu is up would read as two actions from one click. Only the
            // press is handled here — moves still reach the rows, and a press
            // anywhere else in the window is the root view's outside-dismiss
            // (it runs the tray's reported close handle).
            let press = matches!(
                event,
                DispatchedEvent::MouseDown { .. } | DispatchedEvent::MouseUp { .. }
            );
            let inside_row = pointer_position(event).is_some_and(|at| contains(bounds, at));
            if press && inside_row {
                self.ui.borrow_mut().close_menu();
                return true;
            }
        }

        if let Some(child) = self.child.as_mut() {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }

        let on_click = self.on_click.clone();
        let mut run = move || {
            if let Some(cb) = on_click.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut run)
    }
}

/// A 32pt sidebar action row (the "New conversation" entry point): a leading
/// glyph and a label, hovering like a group row.
///
/// Its painted card is inset from the row's own box by [`SIDEBAR_PAINT_INSET`]
/// like the list rows, so it lines up with them; a caller that wants the row to
/// span the sidebar column edge to edge passes [`Self::with_paint_inset`] zero.
pub struct SidebarActionRow {
    icon: &'static str,
    label: String,
    label_token: ColorToken,
    /// How far the painted card is inset from the row's own box on each side.
    paint_inset: f32,
    hovered: bool,
    hover_key: Option<(Rc<RefCell<Option<String>>>, String)>,
    on_click: Option<Rc<RefCell<dyn FnMut()>>>,
    child: Option<Box<dyn Element>>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarActionRow {
    pub fn new(icon: &'static str, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            label_token: ColorToken::Text,
            paint_inset: SIDEBAR_PAINT_INSET,
            hovered: false,
            hover_key: None,
            on_click: None,
            child: None,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    /// How far the painted card is inset from the row's own box on each side.
    ///
    /// The list rows are the default ([`SIDEBAR_PAINT_INSET`]); a sidebar-level
    /// action (the "New conversation" row under the search field) passes zero so
    /// its card fills the column's content width. The content keeps the list
    /// rows' inset whichever card it is painted on, so the glyph and label line
    /// up with the rows' glyphs and labels either way.
    pub fn with_paint_inset(mut self, inset: f32) -> Self {
        self.paint_inset = inset.max(0.0);
        self
    }

    pub fn with_hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// The label's ink; the default is the primary text colour.
    pub fn with_label_color(mut self, token: ColorToken) -> Self {
        self.label_token = token;
        self
    }

    pub fn with_hover_cell(
        mut self,
        cell: Rc<RefCell<Option<String>>>,
        key: impl Into<String>,
    ) -> Self {
        self.hover_key = Some((cell, key.into()));
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.child.is_some() {
            return;
        }
        let leading = sidebar_leading_slot(
            Icon::new(self.icon)
                .with_size(SIDEBAR_HEADER_ICON)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
            SIDEBAR_HEADER_ICON,
        );
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(SIDEBAR_LABEL_GAP)
            .with_child(leading)
            .with_child(
                Text::new(self.label.clone())
                    .with_font_size(SIDEBAR_TITLE_SIZE)
                    .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
                    .with_max_lines(1)
                    .with_theme_color(self.label_token, app)
                    .finish(),
            )
            .finish();
        self.child = Some(
            Container::new(row)
                .with_padding(EdgeInsets::new(
                    // The content sits where a list row's content sits: the
                    // rows paint their card 3 in from the sidebar edge and pad
                    // their content 9 in from the card, so an edge-to-edge card
                    // pads 12 to keep the glyphs in one column.
                    SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_START,
                    (SIDEBAR_ROW_HEIGHT - SIDEBAR_LEADING_SLOT_H) / 2.0,
                    SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_END,
                    (SIDEBAR_ROW_HEIGHT - SIDEBAR_LEADING_SLOT_H) / 2.0,
                ))
                .finish(),
        );
    }
}

impl Element for SidebarActionRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let size = self.child.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        if let Some((cell, key)) = &self.hover_key {
            let hovered = ctx.hovered(bounds);
            let mut cell = cell.borrow_mut();
            if hovered {
                *cell = Some(key.clone());
            } else if cell.as_deref() == Some(key.as_str()) {
                *cell = None;
            }
        }
        let card = rectf(
            origin.x + self.paint_inset,
            origin.y,
            (size.x - 2.0 * self.paint_inset).max(0.0),
            size.y,
        );
        if let Some(renderer) = ctx.renderer.as_mut() {
            if self.hovered {
                renderer.fill_rounded_rect(
                    card,
                    app.theme.color(ColorToken::Hover),
                    SIDEBAR_ROW_RADIUS,
                );
            }
            renderer.clip_rect(card);
        }
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let _ = app;
        let Some(bounds) = self.bounds() else {
            return false;
        };
        let on_click = self.on_click.clone();
        let mut run = move || {
            if let Some(cb) = on_click.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut run)
    }
}

impl SidebarActionRow {
    /// Box the row for a parent's child list.
    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

/// One row of the session row's tray, supplied by the app so the app owns the
/// labels and their availability (DSH's session context menu is assembled from
/// registered contributions the same way).
///
/// A row is actionable: separators are not rows of their own but a
/// `separator_before` flag, so the keyboard walk never lands on one and every
/// [`SidebarMenuItem::run`] fires exactly once per activation.
#[derive(Clone)]
pub struct SidebarMenuItem {
    /// The row's label, e.g. "Rename conversation".
    pub label: String,
    /// The atlas glyph the row leads with.
    pub icon: &'static str,
    /// Destructive rows draw in the error colour (Goble's Delete).
    pub danger: bool,
    /// Draw a hairline separator above this row (DSH's menu groups).
    pub separator_before: bool,
    /// The action the row runs when clicked or Entered.
    pub run: Rc<RefCell<dyn FnMut()>>,
}

impl SidebarMenuItem {
    pub fn new(label: impl Into<String>, icon: &'static str, run: impl FnMut() + 'static) -> Self {
        Self {
            label: label.into(),
            icon,
            danger: false,
            separator_before: false,
            run: Rc::new(RefCell::new(run)),
        }
    }

    /// Draw the row as destructive (the error colour).
    pub fn with_danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Start a new visual group above this row with a hairline.
    pub fn with_separator_before(mut self, separator_before: bool) -> Self {
        self.separator_before = separator_before;
        self
    }
}

/// One session row: leading status slot, title, and a trailing cluster that
/// swaps between the resting cells (harness mark, relative time, pin) and the
/// action button while the row is hovered or its tray is open.
pub struct SidebarSessionRow {
    id: String,
    group: String,
    title: String,
    time_label: String,
    status: ConversationStatus,
    /// The directory the conversation works in, shown on the hover card.
    directory: Option<String>,
    /// The row's raw RFC3339 `updated_at`, for the hover card's long relative
    /// time (the row itself uses the compact label).
    updated_at: Option<String>,
    harness: String,
    remote: bool,
    starred: bool,
    /// Archived rows keep the row's geometry but read as history: muted title,
    /// blank leading slot, no pin, no Star row, and the app blocks opening them.
    archived: bool,
    /// The tray rows the app supplies (labels, icons, separators, actions).
    /// Empty means the legacy Star/Delete pair built from the callbacks below.
    menu_items: Vec<SidebarMenuItem>,
    /// The window's height, for the tray's flip-above clamp (0 = unknown).
    viewport_height: f32,
    selected: bool,
    hovered: bool,
    focus: bool,
    hover_key: Option<(Rc<RefCell<Option<String>>>, String)>,
    drag: Option<Rc<RefCell<SidebarDrag>>>,
    drop_handler: Option<Rc<RefCell<dyn FnMut(&str, &str, bool)>>>,
    ui: Rc<RefCell<AgentCardUi>>,
    on_click: Option<Rc<RefCell<dyn FnMut()>>>,
    on_toggle_star: Option<Rc<RefCell<dyn FnMut()>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut()>>>,
    /// The hover strip's Pin control: toggle the conversation's pin through the
    /// app's durable action. Absent on a row that offers no pin.
    on_quick_pin: Option<Rc<RefCell<dyn FnMut()>>>,
    /// The hover strip's Archive control: archive the conversation, or restore
    /// it when the row is archived. Absent on a row that offers no archive.
    on_quick_archive: Option<Rc<RefCell<dyn FnMut()>>>,
    child: Option<Box<dyn Element>>,
    tray: Option<Rc<RefCell<Box<dyn Element>>>>,
    tray_size: Option<Vector2F>,
    /// The hover card, built when its dwell has elapsed.
    hover_card: Option<Rc<RefCell<Box<dyn Element>>>>,
    hover_card_size: Option<Vector2F>,
    /// The row's measured title cell, so the marquee knows its travel range.
    title_width: f32,
    row_size: Option<Vector2F>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarSessionRow {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        time_label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            group: String::new(),
            title: title.into(),
            time_label: time_label.into(),
            status: ConversationStatus::default(),
            directory: None,
            updated_at: None,
            harness: "goble".to_string(),
            remote: false,
            starred: false,
            archived: false,
            menu_items: Vec::new(),
            viewport_height: 0.0,
            selected: false,
            hovered: false,
            focus: false,
            hover_key: None,
            drag: None,
            drop_handler: None,
            ui: Rc::new(RefCell::new(AgentCardUi::default())),
            on_click: None,
            on_toggle_star: None,
            on_delete: None,
            on_quick_pin: None,
            on_quick_archive: None,
            child: None,
            tray: None,
            tray_size: None,
            hover_card: None,
            hover_card_size: None,
            title_width: 0.0,
            row_size: None,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }

    pub fn with_status(mut self, status: ConversationStatus) -> Self {
        self.status = status;
        self
    }

    /// The directory the hover card shows. Empty draws no path line.
    pub fn with_directory(mut self, directory: impl Into<String>) -> Self {
        let directory = directory.into();
        self.directory = (!directory.trim().is_empty()).then_some(directory);
        self
    }

    /// The row's raw RFC3339 update stamp, so the hover card can word its time
    /// the way the web does ("1h ago") while the row keeps the compact label.
    pub fn with_updated_at(mut self, updated_at: impl Into<String>) -> Self {
        let updated_at = updated_at.into();
        self.updated_at = (!updated_at.trim().is_empty()).then_some(updated_at);
        self
    }

    pub fn with_harness(mut self, harness: impl Into<String>) -> Self {
        self.harness = harness.into();
        self
    }

    pub fn with_remote(mut self, remote: bool) -> Self {
        self.remote = remote;
        self
    }

    pub fn with_starred(mut self, starred: bool) -> Self {
        self.starred = starred;
        self
    }

    /// Draw the row as archived: muted title, blank leading slot, no resting
    /// pin. The app also leaves the Star and Archive/Unarchive rows out of
    /// [`Self::with_menu_items`] and blocks opening the row.
    pub fn with_archived(mut self, archived: bool) -> Self {
        self.archived = archived;
        self
    }

    /// The tray rows, in order. Replaces the legacy Star/Delete pair; the app
    /// owns every label and whether it is offered (archived rows show no Star,
    /// for example).
    pub fn with_menu_items(mut self, items: Vec<SidebarMenuItem>) -> Self {
        self.menu_items = items;
        self
    }

    /// Append one tray row (see [`Self::with_menu_items`]).
    pub fn with_menu_item(mut self, item: SidebarMenuItem) -> Self {
        self.menu_items.push(item);
        self
    }

    /// The window's height, so a tray opened by a row near the bottom flips
    /// above the row instead of running under the window's edge (0 = unknown,
    /// always opens below).
    pub fn with_viewport_height(mut self, height: f32) -> Self {
        self.viewport_height = height;
        self
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn with_hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Draw the keyboard focus ring.
    pub fn with_focus(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    pub fn with_ui(mut self, ui: Rc<RefCell<AgentCardUi>>) -> Self {
        self.ui = ui;
        self
    }

    pub fn with_hover_cell(
        mut self,
        cell: Rc<RefCell<Option<String>>>,
        key: impl Into<String>,
    ) -> Self {
        self.hover_key = Some((cell, key.into()));
        self
    }

    pub fn with_drag(mut self, drag: Rc<RefCell<SidebarDrag>>) -> Self {
        self.drag = Some(drag);
        self
    }

    /// Called on drop with `(source id, target id, after target)`.
    pub fn with_drop_handler<F>(mut self, handler: F) -> Self
    where
        F: FnMut(&str, &str, bool) + 'static,
    {
        self.drop_handler = Some(Rc::new(RefCell::new(handler)));
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_star<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_star = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The hover strip's Pin control (DSH's `PinSessionRowButton`): toggle the
    /// conversation's pin. Goble's pin is the store's durable `chats.starred`,
    /// which is also the Starred section's membership, so the quick control and
    /// the tray's Star row are one action.
    pub fn with_on_quick_pin<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_quick_pin = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The hover strip's Archive control (DSH's `ArchiveSessionRowButton`): an
    /// archived row's control is the restore, so the app passes the action that
    /// matches the row's state.
    pub fn with_on_quick_archive<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_quick_archive = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The hover strip's controls, left to right, in DSH's order: the
    /// context-menu trigger, the Archive toggle, then the Pin toggle. A control
    /// whose action the app did not supply is not drawn (DSH's slot list
    /// resolves the same way when access refuses a toggle).
    fn quick_kinds(&self) -> Vec<SessionQuick> {
        let mut kinds = vec![SessionQuick::More];
        if self.on_quick_archive.is_some() {
            kinds.push(SessionQuick::Archive);
        }
        if self.on_quick_pin.is_some() && !self.archived {
            kinds.push(SessionQuick::Pin);
        }
        kinds
    }

    /// The trailing cell's width this frame: the resting cluster at rest, the
    /// action strip's reserved width while it is shown. Hover therefore narrows
    /// the title cell exactly as DSH's flex row does (the resting cells drop and
    /// the strip takes their room), while the row stays 32pt and its neighbours
    /// never move.
    fn trailing_slot(&self) -> f32 {
        if self.hovered || self.ui.borrow().menu_open {
            sidebar_quick_strip_width(self.quick_kinds().len())
        } else {
            trailing_slot_width(&self.time_label, self.remote, self.starred)
        }
    }

    /// The glyph box and the hit box of each strip control, left to right. The
    /// glyphs sit inside the reserved strip; the 24pt hit boxes are centred on
    /// them, spilling 4pt into the gaps and never into a neighbour.
    fn quick_action_boxes(&self) -> Vec<(SessionQuick, RectF, RectF)> {
        let (Some(origin), Some(size)) = (self.origin, self.size) else {
            return Vec::new();
        };
        if !(self.hovered || self.ui.borrow().menu_open) {
            return Vec::new();
        }
        let kinds = self.quick_kinds();
        let count = kinds.len();
        let right = origin.x() + size.x - SIDEBAR_PAINT_INSET - SIDEBAR_ROW_PADDING_END;
        let mut boxes = Vec::with_capacity(count);
        for (index, kind) in kinds.into_iter().enumerate() {
            let from_right = (count - 1 - index) as f32;
            let glyph_x =
                right - SIDEBAR_QUICK_SLOT - from_right * (SIDEBAR_QUICK_SLOT + SIDEBAR_QUICK_GAP);
            let glyph = rectf(
                glyph_x,
                origin.y() + (size.y - SIDEBAR_LEADING_SLOT_H) / 2.0,
                SIDEBAR_QUICK_SLOT,
                SIDEBAR_LEADING_SLOT_H,
            );
            let hit = rectf(
                glyph_x + (SIDEBAR_QUICK_SLOT - SIDEBAR_QUICK_HIT) / 2.0,
                origin.y() + (size.y - SIDEBAR_QUICK_HIT) / 2.0,
                SIDEBAR_QUICK_HIT,
                SIDEBAR_QUICK_HIT,
            );
            boxes.push((kind, glyph, hit));
        }
        boxes
    }

    /// The tray rows, in their order. The explicit app-supplied list wins; the
    /// legacy callback pair (Star + Delete) is kept for the preview example and
    /// callers that predate the generic menu, with Star offered only on a listed
    /// (non-archived) row.
    fn menu_items(&self) -> Vec<SidebarMenuItem> {
        if !self.menu_items.is_empty() {
            return self.menu_items.clone();
        }
        let mut items = Vec::new();
        if let Some(on_toggle_star) = self.on_toggle_star.clone() {
            if !self.archived {
                let starred = self.starred;
                items.push(SidebarMenuItem::new(
                    if starred { "Unstar" } else { "Star" },
                    if starred {
                        SIDEBAR_ICON_PIN_FILL
                    } else {
                        SIDEBAR_ICON_PIN_OUTLINE
                    },
                    move || (on_toggle_star.borrow_mut())(),
                ));
            }
        }
        if let Some(on_delete) = self.on_delete.clone() {
            items.push(
                SidebarMenuItem::new("Delete conversation", "trash-02", move || {
                    (on_delete.borrow_mut())()
                })
                .with_danger(true),
            );
        }
        items
    }

    /// The actionable row a highlight index lands on (all tray rows are
    /// actionable; separators are flags on them, not rows).
    fn menu_row(&self, index: usize) -> usize {
        let count = self.menu_items().len();
        if count == 0 || index >= count {
            0
        } else {
            index
        }
    }

    fn tray_bounds(&self) -> Option<RectF> {
        let (origin, size) = (self.origin?, self.size?);
        let tray_size = self.tray_size?;
        self.tray.as_ref()?;
        // Open below the row when the tray fits the window; otherwise flip
        // above it (and clamp to the window) so a row low in the sidebar never
        // opens a tray the user cannot reach. A zero viewport height means
        // "unknown": open below, as before.
        let below = origin.y() + size.y + TRAY_GAP;
        let fits_below =
            self.viewport_height <= 0.0 || below + tray_size.y <= self.viewport_height - 4.0;
        let y = if fits_below {
            below
        } else {
            (origin.y() - TRAY_GAP - tray_size.y).max(4.0)
        };
        let x = (origin.x() + size.x - SIDEBAR_PAINT_INSET - tray_size.x).max(SIDEBAR_PAINT_INSET);
        Some(rectf(x, y, tray_size.x, tray_size.y))
    }

    /// The 4pt corridor between the row and its tray, on whichever side the
    /// tray opened: it stays part of the tray's hit area (and of the open-menu
    /// surface the root view reports), so walking the pointer from the dots down
    /// onto the tray never leaves the menu.
    fn tray_corridor(&self) -> Option<RectF> {
        let bounds = self.tray_bounds()?;
        let row_bottom = match (self.origin, self.size) {
            (Some(origin), Some(size)) => origin.y() + size.y,
            _ => bounds.min_y(),
        };
        let (bridge_y, bridge_h) = if bounds.min_y() >= row_bottom {
            (bounds.min_y() - TRAY_GAP, TRAY_GAP)
        } else {
            (bounds.max_y(), TRAY_GAP)
        };
        Some(rectf(bounds.min_x(), bridge_y, bounds.width(), bridge_h))
    }

    /// The close handle the window's open-menu registry runs when another menu
    /// opens, Escape is pressed, or a press lands outside: it is the tray's own
    /// close, so every dismissal path lands here.
    fn tray_close(&self) -> MenuClose {
        let ui = Rc::clone(&self.ui);
        Rc::new(RefCell::new(move || ui.borrow_mut().close_menu()))
    }

    /// The dots' hit box, which the registry treats as the tray's anchor: a
    /// press there toggles the tray instead of reading as an outside dismiss.
    fn tray_anchor(&self) -> Option<RectF> {
        self.quick_action_boxes()
            .into_iter()
            .find(|(kind, _, _)| *kind == SessionQuick::More)
            .map(|(_, _, hit)| hit)
    }

    fn pointer_on_tray(&self, position: Vector2F) -> bool {
        let Some(bounds) = self.tray_bounds() else {
            return false;
        };
        contains(bounds, position)
            || self
                .tray_corridor()
                .is_some_and(|corridor| contains(corridor, position))
    }

    fn menu_takes_event(&self, event: &DispatchedEvent) -> bool {
        match pointer_position(event) {
            None => true,
            Some(position) => self.pointer_on_tray(position),
        }
    }

    /// The tray's keys, answered only while it is open.
    fn menu_key(&mut self, event: &DispatchedEvent) -> bool {
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if modifiers.command || modifiers.ctrl || modifiers.alt || !self.ui.borrow().menu_open {
            return false;
        }
        let count = self.menu_items().len();
        if count == 0 {
            return false;
        }
        match key.as_str() {
            "ArrowUp" | "ArrowDown" => {
                let stored = self.ui.borrow().highlight;
                let next = match (key.as_str(), stored >= count) {
                    // Nothing is highlighted yet: Down enters at the first row,
                    // Up enters at the last (DSH's menu walk).
                    ("ArrowDown", true) => 0,
                    ("ArrowUp", true) => count - 1,
                    ("ArrowDown", false) => (stored + 1).min(count - 1),
                    (_, false) => stored.saturating_sub(1),
                    _ => unreachable!(),
                };
                self.ui.borrow_mut().highlight = next;
                true
            }
            "Enter" => {
                let highlighted = self.menu_row(self.ui.borrow().highlight);
                self.run_menu_row(highlighted);
                true
            }
            "Escape" => {
                self.ui.borrow_mut().close_menu();
                true
            }
            _ => false,
        }
    }

    fn run_menu_row(&self, index: usize) {
        self.ui.borrow_mut().close_menu();
        let items = self.menu_items();
        if let Some(item) = items.get(index) {
            (item.run.borrow_mut())();
        }
    }

    fn status_glyph(&self, app: &AppContext) -> Box<dyn Element> {
        // Archived rows lead with a blank slot: no status dot or ring reads a
        // history row as live.
        if self.archived {
            return Empty::new()
                .with_size(vec2f(SIDEBAR_LEADING_SLOT_W, SIDEBAR_LEADING_SLOT_H))
                .finish();
        }
        match self.status {
            ConversationStatus::Running => icon_leading_slot(
                Icon::new(SIDEBAR_ICON_STATUS_RUNNING)
                    .with_size(SIDEBAR_RUNNING_RING)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
                SIDEBAR_RUNNING_RING,
            ),
            ConversationStatus::Waiting => icon_leading_slot(
                sidebar_status_dot(ColorToken::Warning, app),
                SIDEBAR_STATUS_DOT,
            ),
            ConversationStatus::Error => icon_leading_slot(
                sidebar_status_dot(ColorToken::Error, app),
                SIDEBAR_STATUS_DOT,
            ),
            ConversationStatus::Success
            | ConversationStatus::Stopped
            | ConversationStatus::Default => Empty::new()
                .with_size(vec2f(SIDEBAR_LEADING_SLOT_W, SIDEBAR_LEADING_SLOT_H))
                .finish(),
        }
    }

    fn ensure_root(&mut self, app: &AppContext, title_width: Option<f32>) {
        if self.child.is_some() {
            return;
        }
        let ui = self.ui.borrow();
        let menu_open = ui.menu_open;
        let highlight = ui.highlight;
        drop(ui);

        // The trailing cell is one fixed width in every state: at rest it draws
        // today's cluster plus a spacer up to the slot; hovered and with the
        // tray open it is the action's 16pt layout footprint (the 24pt hit
        // target is painted from `action_bounds` and never participates in
        // layout). Content is therefore always 20pt tall and the row always 32,
        // and the title cell — with its baked ellipsis — never reflows.
        let slot = self.trailing_slot();
        let trailing: Box<dyn Element> = if self.hovered || menu_open {
            // The glyph itself is painted in `paint`, where the pointer's own
            // position is known: Muted at rest, Text while the button is hovered.
            Empty::new()
                .with_size(vec2f(slot, SIDEBAR_LEADING_SLOT_H))
                .finish()
        } else {
            let mut cluster = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(SIDEBAR_TRAILING_GAP);
            if self.remote {
                cluster = cluster.with_child(
                    Icon::new(SIDEBAR_ICON_CLOUD)
                        .with_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
            cluster = cluster.with_child(sidebar_time_text(self.time_label.clone(), app));
            if self.starred && !self.archived {
                cluster = cluster.with_child(
                    Icon::new(SIDEBAR_ICON_PIN_FILL)
                        .with_size(SIDEBAR_PIN_SIZE)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
            let cluster = cluster.finish();
            let gap = (slot - resting_cluster_width(&self.time_label, self.remote, self.starred))
                .max(0.0);
            if gap > 0.0 {
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Empty::new()
                            .with_size(vec2f(gap, SIDEBAR_LEADING_SLOT_H))
                            .finish(),
                    )
                    .with_child(cluster)
                    .finish()
            } else {
                cluster
            }
        };

        let marquee = {
            let ui = self.ui.borrow();
            self.hovered && ui.marquee_offset > 0.0
        };
        let title: Box<dyn Element> = if marquee {
            SidebarMarqueeTitle {
                text: self.title.clone(),
                offset: self.ui.borrow().marquee_offset,
                width: self.title_width,
                size: None,
                origin: None,
            }
            .finish()
        } else {
            Text::new(title_width.map_or_else(
                || self.title.clone(),
                |width| {
                    truncate_with_ellipsis(
                        &self.title,
                        SIDEBAR_TITLE_SIZE,
                        SIDEBAR_TITLE_LINE,
                        width,
                    )
                },
            ))
            .with_font_size(SIDEBAR_TITLE_SIZE)
            .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
            .with_max_lines(1)
            .with_theme_color(
                if self.archived {
                    ColorToken::Muted
                } else {
                    ColorToken::Text
                },
                app,
            )
            .finish()
        };
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(self.status_glyph(app))
            .with_child(
                Expanded::new(
                    Padding::new(title, EdgeInsets::new(SIDEBAR_LABEL_GAP, 0.0, 6.0, 0.0)).finish(),
                )
                .finish(),
            )
            .with_child(trailing)
            .finish();
        self.child = Some(
            Container::new(row)
                .with_padding(EdgeInsets::new(
                    SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_START,
                    (SIDEBAR_ROW_HEIGHT - SIDEBAR_LEADING_SLOT_H) / 2.0,
                    SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_END,
                    (SIDEBAR_ROW_HEIGHT - SIDEBAR_LEADING_SLOT_H) / 2.0,
                ))
                .finish(),
        );

        if menu_open {
            let items = self.menu_items();
            let mut rows = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, item) in items.iter().enumerate() {
                if index > 0 && item.separator_before {
                    rows = rows.with_child(Box::new(SidebarTraySeparator {
                        size: None,
                        origin: None,
                    }));
                }
                let ui = Rc::clone(&self.ui);
                let run = Rc::clone(&item.run);
                rows =
                    rows.with_child(sidebar_menu_row(app, item, highlight == index, move || {
                        ui.borrow_mut().close_menu();
                        (run.borrow_mut())();
                    }));
            }
            let tray = sidebar_menu_tray(app, rows.finish(), &items);
            self.tray = Some(Rc::new(RefCell::new(tray)));
        }
        if self.ui.borrow().hover_card_open {
            let card_time = self
                .updated_at
                .as_deref()
                .map(sidebar_relative_time_long)
                .unwrap_or_else(|| self.time_label.clone());
            self.hover_card = Some(Rc::new(RefCell::new(sidebar_hover_card(
                app,
                &self.title,
                self.directory.as_deref(),
                &card_time,
                self.status,
                self.archived,
            ))));
        }
    }
}

/// Box a glyph into the leading slot at its own size.
fn icon_leading_slot(glyph: Box<dyn Element>, size: f32) -> Box<dyn Element> {
    let horizontal = ((SIDEBAR_LEADING_SLOT_W - size) / 2.0).max(0.0);
    let vertical = ((SIDEBAR_LEADING_SLOT_H - size) / 2.0).max(0.0);
    Container::new(glyph)
        .with_padding(EdgeInsets::new(horizontal, vertical, horizontal, vertical))
        .finish()
}

impl SidebarSessionRow {
    /// Box the row for a parent's child list.
    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for SidebarSessionRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // One trailing width in every state, so the title cell (and the baked
        // ellipsis built from `title_width`) cannot reflow between rest, hover
        // and tray-open.
        let trailing_width = self.trailing_slot();
        let title_width = if constraint.max.x.is_finite() {
            Some(
                (constraint.max.x
                    - 2.0 * SIDEBAR_PAINT_INSET
                    - SIDEBAR_ROW_PADDING_START
                    - SIDEBAR_ROW_PADDING_END
                    - SIDEBAR_LEADING_SLOT_W
                    - SIDEBAR_LABEL_GAP
                    - 6.0
                    - trailing_width)
                    .max(0.0),
            )
        } else {
            None
        };
        self.title_width = title_width.unwrap_or(0.0);
        // The hover card's dwell and the title's marquee both run on the frame
        // clock the row's hover keeps awake (`RootView::wants_animation`).
        {
            let mut ui = self.ui.borrow_mut();
            if self.hovered && !ui.menu_open {
                let now = std::time::Instant::now();
                let since = *ui.hover_card_since.get_or_insert(now);
                if now.duration_since(since).as_millis() >= 800 {
                    ui.hover_card_open = true;
                }
                let overflow = measure_text(
                    &self.title,
                    SIDEBAR_TITLE_SIZE,
                    SIDEBAR_TITLE_LINE,
                    f32::INFINITY,
                )
                .x - self.title_width.max(0.0).min(f32::MAX);
                if overflow > 8.0 {
                    let last = ui.marquee_last.unwrap_or(now);
                    let dt = now.duration_since(last).as_secs_f32();
                    ui.marquee_offset = (ui.marquee_offset + dt * 30.0).min(overflow + 8.0);
                    ui.marquee_last = Some(now);
                }
            } else {
                ui.hover_card_since = None;
                ui.hover_card_open = false;
                ui.marquee_offset = 0.0;
                ui.marquee_last = None;
            }
        }
        self.ensure_root(app, title_width);
        let size = self.child.as_mut().unwrap().layout(constraint, ctx, app);
        self.row_size = Some(size);
        self.hover_card_size = None;
        if self.hover_card.is_some() {
            let card_size = self.hover_card.as_ref().unwrap().borrow_mut().layout(
                SizeConstraint::loose(vec2f(HOVER_CARD_WIDTH, 400.0)),
                ctx,
                app,
            );
            self.hover_card_size = Some(card_size);
        }
        self.size = Some(size);
        self.tray_size = None;
        if let Some(tray) = self.tray.as_ref() {
            let tray_size = tray.borrow_mut().layout(
                SizeConstraint::loose(vec2f(SIDEBAR_MENU_MAX_WIDTH, 400.0)),
                ctx,
                app,
            );
            self.tray_size = Some(tray_size);
        }
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        if let Some((cell, key)) = &self.hover_key {
            let hovered = ctx.hovered(bounds);
            let mut cell = cell.borrow_mut();
            if hovered {
                *cell = Some(key.clone());
            } else if cell.as_deref() == Some(key.as_str()) {
                *cell = None;
            }
        }
        let menu_open = self.ui.borrow().menu_open;
        let card = rectf(
            origin.x + SIDEBAR_PAINT_INSET,
            origin.y,
            (size.x - 2.0 * SIDEBAR_PAINT_INSET).max(0.0),
            size.y,
        );
        // The row a drag was lifted from reads as a raised card above the list.
        let drag_source = self.drag.as_ref().is_some_and(|drag| {
            let drag = drag.borrow();
            drag.active && drag.press.as_ref().is_some_and(|press| press.id == self.id)
        });
        if let Some(renderer) = ctx.renderer.as_mut() {
            let fill = if drag_source {
                Some(ColorToken::SurfaceRaised)
            } else if self.selected || menu_open {
                Some(ColorToken::Selected)
            } else if self.hovered {
                Some(ColorToken::Hover)
            } else {
                None
            };
            if let Some(token) = fill {
                renderer.fill_rounded_rect(card, app.theme.color(token), SIDEBAR_ROW_RADIUS);
            }
            if drag_source {
                renderer.stroke_rect(
                    card,
                    app.theme.color(ColorToken::Border),
                    1.0,
                    SIDEBAR_ROW_RADIUS,
                );
            }
            if self.focus {
                renderer.stroke_rect(
                    rectf(
                        card.min_x() + 1.0,
                        card.min_y() + 1.0,
                        card.width() - 2.0,
                        card.height() - 2.0,
                    ),
                    app.theme.color(ColorToken::Focus),
                    2.0,
                    SIDEBAR_ROW_RADIUS,
                );
            }
            renderer.clip_rect(card);
        }
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
        }
        // The hover strip's glyphs: muted until the pointer is on one, primary
        // while it is (DSH's `iconButton` ink ramp). Painted after the row's own
        // content so they sit over the cell the title keeps clear of.
        for (kind, glyph, hit) in self.quick_action_boxes() {
            let (icon, icon_size) = match kind {
                SessionQuick::More => (SIDEBAR_ICON_ELLIPSIS, SIDEBAR_ACTION_SLOT),
                SessionQuick::Archive => (
                    if self.archived {
                        SIDEBAR_ICON_UNARCHIVE
                    } else {
                        SIDEBAR_ICON_ARCHIVE
                    },
                    SIDEBAR_QUICK_ICON,
                ),
                SessionQuick::Pin => (
                    if self.starred {
                        SIDEBAR_ICON_PIN_FILL
                    } else {
                        SIDEBAR_ICON_PIN_OUTLINE
                    },
                    SIDEBAR_QUICK_ICON,
                ),
            };
            let ink = if ctx.hovered(hit) {
                ColorToken::Text
            } else {
                ColorToken::Muted
            };
            if let Some(renderer) = ctx.renderer.as_mut() {
                let inset = (SIDEBAR_QUICK_SLOT - icon_size) / 2.0;
                renderer.draw_icon(
                    vec2f(glyph.min_x() + inset, glyph.min_y() + inset),
                    icon,
                    icon_size,
                    app.theme.color(ink),
                );
            }
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }
        // The insert marker: a 2pt rule plus the DSH arrow head.
        let marker = self
            .drag
            .as_ref()
            .and_then(|drag| drag.borrow().marker_for(&self.id));
        if let Some(after) = marker {
            let y = if after {
                origin.y + size.y - 1.0
            } else {
                origin.y
            };
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rect(
                    rectf(card.min_x(), y - 1.0, card.width(), 2.0),
                    app.theme.color(ColorToken::Focus),
                );
                renderer.draw_icon(
                    vec2f(card.min_x() + 2.0, y - 7.0),
                    SIDEBAR_ICON_TRIANGLE_RIGHT,
                    14.0,
                    app.theme.color(ColorToken::Focus),
                );
            }
        }
        // The hover card sits beside the row, over the main content, queued
        // into the frame's top layer with the tray. Its left edge is 8pt off the
        // row's painted fill (not its layout box), matching the web's card.
        if let (Some(card), Some(card_size)) = (self.hover_card.as_ref(), self.hover_card_size) {
            let at = vec2f(origin.x + size.x - SIDEBAR_PAINT_INSET + 8.0, origin.y);
            card.borrow_mut().paint(at, ctx, app);
            let _ = card_size;
            app.hover_chips
                .borrow_mut()
                .push_shared(at, Rc::clone(card));
        }
        // The tray, queued into the frame's top layer so nothing painted after
        // this row can cover it. While it is up it reports itself and its
        // surface to the window's open-menu registry, and paints inside a menu
        // surface so its own hover survives the gate the root view applies to
        // everything the menu covers.
        if let Some(tray_bounds) = self.tray_bounds() {
            if let Some(tray) = self.tray.as_ref() {
                let at = vec2f(tray_bounds.min_x(), tray_bounds.min_y());
                let mut surfaces = vec![tray_bounds];
                surfaces.extend(self.tray_corridor());
                app.open_menus.report(
                    SESSION_TRAY_MENU_ID,
                    self.tray_close(),
                    surfaces,
                    MenuPointer::Paint,
                );
                if let Some(anchor) = self.tray_anchor() {
                    app.open_menus.report_anchor(SESSION_TRAY_MENU_ID, anchor);
                }
                ctx.enter_menu_surface();
                tray.borrow_mut().paint(at, ctx, app);
                app.hover_chips
                    .borrow_mut()
                    .push_shared(at, Rc::clone(tray));
                ctx.leave_menu_surface();
            }
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if self.child.is_none() {
            self.ensure_root(app, None);
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };

        if self.menu_key(event) {
            return true;
        }

        // The hover strip's own controls take the press before the row does:
        // the dots open or close the tray, Archive and Pin run their durable
        // actions. Only the press acts (the release is consumed with it), so the
        // row's own click never fires from the same click and the tray never
        // toggles twice.
        if self.hovered || self.ui.borrow().menu_open {
            for (kind, _, hit) in self.quick_action_boxes() {
                if !pointer_inside(event, hit) {
                    continue;
                }
                match event {
                    DispatchedEvent::MouseDown { .. } => {
                        match kind {
                            SessionQuick::More => {
                                let open = self.ui.borrow().menu_open;
                                if open {
                                    // Closing through the registry runs the same
                                    // close handle every other dismissal path
                                    // uses (and drops the ui borrow first, so the
                                    // handle can take it).
                                    app.open_menus.close();
                                } else {
                                    app.open_menus.open(SESSION_TRAY_MENU_ID, self.tray_close());
                                    let mut ui = self.ui.borrow_mut();
                                    if !ui.menu_open {
                                        ui.menu_open = true;
                                        // DSH opens the menu unfilled; the first
                                        // arrow key enters the row walk.
                                        ui.highlight = NO_HIGHLIGHT;
                                    }
                                }
                            }
                            SessionQuick::Archive => {
                                if let Some(callback) = self.on_quick_archive.clone() {
                                    (callback.borrow_mut())();
                                }
                            }
                            SessionQuick::Pin => {
                                if let Some(callback) = self.on_quick_pin.clone() {
                                    (callback.borrow_mut())();
                                }
                            }
                        }
                        return true;
                    }
                    DispatchedEvent::MouseUp { .. } => return true,
                    _ => {}
                }
            }
        }
        if self.ui.borrow().menu_open {
            let takes_menu = self.menu_takes_event(event);
            if takes_menu {
                if let Some(tray) = self.tray.as_mut() {
                    if tray.borrow_mut().dispatch_event(event, ctx, app) {
                        return true;
                    }
                }
            }
        }

        // Drag reorder: the press arms the gesture, travel past the slop turns
        // it into a drag, and the release on a row reports the drop.
        if let Some(drag_cell) = self.drag.clone() {
            if let Some(handled) = self.handle_drag(event, bounds, &drag_cell, app) {
                return handled;
            }
        }

        if let Some(child) = self.child.as_mut() {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }

        let on_click = self.on_click.clone();
        let mut run = move || {
            if let Some(cb) = on_click.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut run)
    }
}

impl SidebarSessionRow {
    /// The drag half of `dispatch_event`. Returns `Some(true)` when the event
    /// was consumed by the gesture, `None` when it should fall through to the
    /// row's own click handling.
    fn handle_drag(
        &self,
        event: &DispatchedEvent,
        bounds: RectF,
        drag: &Rc<RefCell<SidebarDrag>>,
        app: &AppContext,
    ) -> Option<bool> {
        // Archived conversations are history, not reorderable rows (DSH gates
        // `draggable` on `!row.archived`): the press never arms a reorder.
        if self.archived {
            return None;
        }
        match event {
            DispatchedEvent::MouseDown { position, .. } if contains(bounds, *position) => {
                let mut state = drag.borrow_mut();
                if !state.active {
                    state.press = Some(SidebarDragPress {
                        id: self.id.clone(),
                        group: self.group.clone(),
                        at: *position,
                    });
                }
                None
            }
            DispatchedEvent::MouseMove { position } => {
                let mut state = drag.borrow_mut();
                let Some(press) = state.press.clone() else {
                    return None;
                };
                if press.id != self.id || !app.pointer_down.get() {
                    return None;
                }
                let dx = position.x - press.at.x;
                let dy = position.y - press.at.y;
                if !state.active && (dx * dx + dy * dy).sqrt() > 4.0 {
                    state.active = true;
                }
                if state.active && contains(bounds, *position) {
                    let origin = self.origin?;
                    let size = self.size?;
                    let after = position.y > origin.y() + size.y / 2.0;
                    state.marker = Some((self.id.clone(), after));
                    return Some(false);
                }
                None
            }
            DispatchedEvent::MouseUp { position, .. } => {
                let mut state = drag.borrow_mut();
                if state.active {
                    if contains(bounds, *position) {
                        let source = state
                            .press
                            .as_ref()
                            .map(|press| press.id.clone())
                            .unwrap_or_default();
                        let after = position.y
                            > self.origin.unwrap_or_default().y()
                                + self.size.unwrap_or_default().y / 2.0;
                        if let Some(handler) = self.drop_handler.as_ref() {
                            (handler.borrow_mut())(&source, &self.id, after);
                        }
                        state.clear();
                        return Some(true);
                    }
                    return None;
                }
                if state
                    .press
                    .as_ref()
                    .is_some_and(|press| press.id == self.id)
                {
                    state.press = None;
                }
                None
            }
            _ => None,
        }
    }
}

/// The hover card's width (DSH's 244pt card).
const HOVER_CARD_WIDTH: f32 = 244.0;

/// The hover card's content inset, inside its [`HOVER_CARD_WIDTH`] box.
const HOVER_CARD_PADDING: f32 = 16.0;

/// Cut `text` to at most `max_lines` wrapped lines at `max_width`, ending the
/// last line in an ellipsis when anything had to go.
///
/// [`Text`] wraps at its measured width but only clamps its *box* height, so a
/// run that needs more lines than the clamp still paints the overflow. The
/// hover card's title wraps two lines the way the web clamps it with CSS, so
/// the cut is made here in the builder: the line count comes from the same
/// wrapper the renderer draws with ([`wrapped_lines`]), and the longest prefix
/// whose wrapped form *plus* the ellipsis still fits the line budget is kept.
fn clamp_to_lines(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    max_lines: usize,
) -> String {
    let lines_for = |candidate: &str| {
        wrapped_lines(
            candidate,
            font_size,
            line_height,
            max_width,
            FontWeight::Regular,
            FontFamily::System,
            false,
        )
        .len()
    };
    if lines_for(text) <= max_lines {
        return text.to_string();
    }
    // The wrap only ever gains lines as the run grows, so the longest prefix
    // that fits with the ellipsis can be binary-searched.
    let chars: Vec<char> = text.chars().collect();
    let fits = |len: usize| {
        let mut candidate: String = chars[..len].iter().collect();
        candidate.push('…');
        lines_for(&candidate) <= max_lines
    };
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let mid = (low + high + 1) / 2;
        if fits(mid) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let mut kept: String = chars[..low].iter().collect();
    kept.push('…');
    kept
}

/// One row's hover card: the full title, the directory it works in, its
/// relative time and its live status. The card is drawn beside the row by the
/// frame's top layer, so it can reach over the main content the way DSH's does.
///
/// The card is a fixed 244pt wide box: the column stretches its children to the
/// content width and the title wraps to two lines (ellipsized past that), so a
/// long conversation name can never widen the card past its canvas slot.
fn sidebar_hover_card(
    app: &AppContext,
    title: &str,
    directory: Option<&str>,
    time_label: &str,
    status: ConversationStatus,
    archived: bool,
) -> Box<dyn Element> {
    let inner_width = HOVER_CARD_WIDTH - 2.0 * HOVER_CARD_PADDING;
    let title_text = clamp_to_lines(
        title,
        SIDEBAR_TITLE_SIZE,
        SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE,
        inner_width,
        2,
    );
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(8.0)
        .with_child(
            Text::new(title_text)
                .with_font_size(SIDEBAR_TITLE_SIZE)
                .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
                .with_max_lines(2)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );
    if let Some(directory) = directory {
        column = column.with_child(
            Text::new(truncate_with_ellipsis(
                &abbreviate_home(directory),
                12.0,
                16.0 / 12.0,
                inner_width,
            ))
            .with_font_size(12.0)
            .with_line_height(16.0 / 12.0)
            .with_max_lines(1)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        );
    }
    column = column.with_child(
        Text::new(time_label.to_string())
            .with_font_size(12.0)
            .with_line_height(16.0 / 12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    let status_label = match status {
        ConversationStatus::Running => Some(("Running", ColorToken::Muted)),
        ConversationStatus::Waiting => Some(("Waiting for you", ColorToken::Warning)),
        // The web's hover card reads the finished state as muted, not as a
        // success badge: the card is context, not a status pill.
        ConversationStatus::Success => Some(("Done", ColorToken::Muted)),
        ConversationStatus::Error => Some(("The last turn failed", ColorToken::Error)),
        _ => None,
    };
    if let Some((label, color)) = status_label {
        let mut line = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_child(sidebar_status_dot(color, app));
        line = line.with_child(
            Text::new(label.to_string())
                .with_font_size(12.0)
                .with_line_height(16.0 / 12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
        column = column.with_child(line.finish());
    }
    if archived {
        column = column.with_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0)
                .with_child(
                    Icon::new("archive")
                        .with_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new("Archived".to_string())
                        .with_font_size(12.0)
                        .with_line_height(16.0 / 12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .finish(),
        );
    }
    ConstrainedBox::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .with_corner_radius(12.0)
            .with_padding(EdgeInsets::new(
                HOVER_CARD_PADDING,
                12.0,
                HOVER_CARD_PADDING,
                12.0,
            ))
            .finish(),
    )
    .with_width(HOVER_CARD_WIDTH)
    .finish()
}

/// The title while its row is hovered: the full run, clipped to the row's
/// title cell and scrolled by the marquee offset (DSH's crawl to the far edge).
struct SidebarMarqueeTitle {
    text: String,
    offset: f32,
    width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarMarqueeTitle {
    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for SidebarMarqueeTitle {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if self.width > 0.0 {
            self.width.min(constraint.max.x)
        } else {
            constraint.max.x
        };
        let size = vec2f(width, SIDEBAR_TITLE_LINE);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.clip_rect(bounds);
            renderer.draw_text(
                vec2f(origin.x - self.offset, origin.y),
                self.text.clone(),
                SIDEBAR_TITLE_SIZE,
                app.theme.color(ColorToken::Text),
                f32::INFINITY,
                SIDEBAR_TITLE_LINE,
            );
            renderer.pop_clip();
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The list wrapper that clears a drag the rows never saw released (the pointer
/// let go over the header, or outside the list entirely).
pub struct SidebarList {
    child: Box<dyn Element>,
    drag: Rc<RefCell<SidebarDrag>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarList {
    pub fn new(child: Box<dyn Element>, drag: Rc<RefCell<SidebarDrag>>) -> Self {
        Self {
            child,
            drag,
            size: None,
            origin: None,
        }
    }
}

impl SidebarList {
    /// Box the wrapper for a parent's child list.
    pub fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for SidebarList {
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if self.child.dispatch_event(event, ctx, app) {
            return true;
        }
        if matches!(event, DispatchedEvent::MouseUp { .. }) {
            let mut drag = self.drag.borrow_mut();
            if drag.active || drag.press.is_some() {
                drag.clear();
            }
        }
        false
    }
}

/// The pointer's position on a mouse event, or `None` for keys/focus/scroll.
fn pointer_position(event: &DispatchedEvent) -> Option<Vector2F> {
    match event {
        DispatchedEvent::MouseDown { position, .. }
        | DispatchedEvent::MouseUp { position, .. }
        | DispatchedEvent::MouseMove { position } => Some(*position),
        _ => None,
    }
}

fn pointer_inside(event: &DispatchedEvent, bounds: RectF) -> bool {
    pointer_position(event).is_some_and(|position| contains(bounds, position))
}

/// Replace a leading home directory with `~`, so a hover card's path reads like
/// the rest of the desktop's paths.
fn abbreviate_home(path: &str) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy();
        if !home.is_empty() {
            if let Some(rest) = path.strip_prefix(home.as_ref()) {
                return format!("~{rest}");
            }
        }
    }
    path.to_string()
}

#[cfg(test)]
mod sidebar_workspaces_tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    fn app() -> AppContext {
        AppContext::default()
    }

    fn texts(commands: &[RenderCommand]) -> Vec<(String, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text, font_size, ..
                } => Some((text.clone(), *font_size)),
                _ => None,
            })
            .collect()
    }

    fn icons(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// DSH's compact buckets: now / 4min / 17h / 2d / 3mo / 1y.
    #[test]
    fn the_compact_relative_time_uses_dsh_buckets() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-17T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let at = |s: &str| sidebar_relative_time_at(now, s);
        assert_eq!(at("2026-11-17T11:59:30Z"), "now");
        assert_eq!(at("2026-11-17T11:56:00Z"), "4min");
        assert_eq!(at("2026-11-16T19:00:00Z"), "17h");
        assert_eq!(at("2026-11-15T12:00:00Z"), "2d");
        assert_eq!(at("2026-08-17T12:00:00Z"), "3mo");
        assert_eq!(at("2025-01-01T12:00:00Z"), "1y");
    }

    #[test]
    fn a_long_title_is_truncated_with_an_ellipsis() {
        let wide = truncate_with_ellipsis("Redesign the sidebar", 14.0, 20.0, 400.0);
        assert_eq!(wide, "Redesign the sidebar");
        let cut = truncate_with_ellipsis("Redesign the sidebar", 14.0, 20.0, 60.0);
        assert!(
            cut.ends_with('…'),
            "the cut title ends in an ellipsis: {cut:?}"
        );
        assert!(cut.chars().count() < "Redesign the sidebar".chars().count());
    }

    /// The group row is 32pt, leads with the stroke folder and swaps it for the
    /// filled disclosure triangle while hovered.
    #[test]
    fn a_group_row_swaps_its_folder_for_the_triangle_on_hover() {
        let app = app();
        let mut rest: Box<dyn Element> = SidebarGroupRow::new("goble", false).finish();
        let resting = render_element(&mut rest, vec2f(240.0, 32.0), &app);
        assert!(icons(&resting)
            .iter()
            .any(|name| name == SIDEBAR_ICON_FOLDER_CLOSE));
        assert!(!icons(&resting)
            .iter()
            .any(|name| name == SIDEBAR_ICON_TRIANGLE_RIGHT));

        let mut hovered: Box<dyn Element> = SidebarGroupRow::new("goble", false)
            .with_hovered(true)
            .finish();
        let drawn = render_element(&mut hovered, vec2f(240.0, 32.0), &app);
        assert!(icons(&drawn)
            .iter()
            .any(|name| name == SIDEBAR_ICON_TRIANGLE_RIGHT));
        assert!(!icons(&drawn)
            .iter()
            .any(|name| name == SIDEBAR_ICON_FOLDER_CLOSE));
        let band = drawn
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    corner_radius,
                    ..
                } if (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01 => Some(*rect),
                _ => None,
            })
            .expect("the hovered group paints its rounded card");
        assert!((band.height() - SIDEBAR_ROW_HEIGHT).abs() < 0.01);
        assert!((band.min_x() - SIDEBAR_PAINT_INSET).abs() < 0.01);
    }

    /// A resting session row draws the 10pt relative time and no action button;
    /// hovering swaps the time for the 24pt ellipsis button and paints the Hover
    /// band; the selected row paints the Selected band.
    #[test]
    fn a_session_row_swaps_its_time_for_the_action_button_on_hover() {
        let app = app();
        let mut rest: Box<dyn Element> =
            SidebarSessionRow::new("c1", "Redesign the sidebar", "4min").finish();
        let resting = render_element(&mut rest, vec2f(240.0, 32.0), &app);
        assert!(texts(&resting)
            .iter()
            .any(|(text, size)| text == "4min" && (*size - 10.0).abs() < 0.01));
        assert!(!icons(&resting)
            .iter()
            .any(|name| name == SIDEBAR_ICON_ELLIPSIS));

        let mut hovered: Box<dyn Element> =
            SidebarSessionRow::new("c1", "Redesign the sidebar", "4min")
                .with_hovered(true)
                .finish();
        let drawn = render_element(&mut hovered, vec2f(240.0, 32.0), &app);
        assert!(icons(&drawn)
            .iter()
            .any(|name| name == SIDEBAR_ICON_ELLIPSIS));
        assert!(!texts(&drawn).iter().any(|(text, _)| text == "4min"));
        assert!(drawn.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == app.theme.color(ColorToken::Hover)
        )));

        let mut selected: Box<dyn Element> =
            SidebarSessionRow::new("c1", "Redesign the sidebar", "4min")
                .with_selected(true)
                .finish();
        let drawn = render_element(&mut selected, vec2f(240.0, 32.0), &app);
        assert!(drawn.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == app.theme.color(ColorToken::Selected)
        )));
    }

    /// The leading slot reports the live status: the running ring for a turn in
    /// flight and the amber dot while the agent waits on the user.
    #[test]
    fn the_leading_slot_reports_the_live_status() {
        let app = app();
        let mut running: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "now")
            .with_status(ConversationStatus::Running)
            .finish();
        let drawn = render_element(&mut running, vec2f(240.0, 32.0), &app);
        assert!(icons(&drawn)
            .iter()
            .any(|name| name == SIDEBAR_ICON_STATUS_RUNNING));

        let mut waiting: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "now")
            .with_status(ConversationStatus::Waiting)
            .finish();
        let drawn = render_element(&mut waiting, vec2f(240.0, 32.0), &app);
        assert!(!icons(&drawn)
            .iter()
            .any(|name| name == SIDEBAR_ICON_STATUS_RUNNING));
        assert!(drawn.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == app.theme.color(ColorToken::Warning)
        )));
    }

    /// The drag insert marker: a 2pt accent rule plus the DSH arrow head.
    #[test]
    fn the_drag_insert_marker_paints_an_accent_rule_and_arrow() {
        let app = app();
        let drag = std::rc::Rc::new(std::cell::RefCell::new(SidebarDrag {
            press: None,
            active: true,
            marker: Some(("c1".to_string(), true)),
        }));
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "now")
            .with_drag(drag)
            .finish();
        let drawn = render_element(&mut row, vec2f(240.0, 32.0), &app);
        // The spec cue colour is the cyan focus/accent cue (web palette.accent).
        let accent = app.theme.color(ColorToken::Focus);
        assert!(drawn.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { rect, color, .. }
                if *color == accent && (rect.height() - 2.0).abs() < 0.01
        )));
        assert!(drawn.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, color, .. }
                if name == SIDEBAR_ICON_TRIANGLE_RIGHT && *color == accent
        )));
    }

    #[test]
    fn the_header_icon_button_is_twenty_eight_points() {
        let app = app();
        let mut button: Box<dyn Element> = SidebarIconButton::new(SIDEBAR_ICON_SLIDERS).finish();
        let size = button.layout(
            crate::elements::SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!((size.x - 28.0).abs() < 0.01 && (size.y - 28.0).abs() < 0.01);
    }

    /// The "New conversation" row: the list rows' 32pt anatomy, but its painted
    /// card spans the whole sidebar column (no 3pt paint inset) so it reads as
    /// the sidebar's own button. Its content still starts where the list rows'
    /// content starts, so the glyphs and labels line up.
    #[test]
    fn the_new_conversation_row_is_a_full_width_rounded_action() {
        let app = app();
        let mut row: Box<dyn Element> =
            SidebarActionRow::new(SIDEBAR_ICON_NEW_CHAT, "New conversation")
                .with_paint_inset(0.0)
                .with_hovered(true)
                .finish();
        let drawn = render_element(&mut row, vec2f(284.0, 40.0), &app);

        let band = drawn
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Hover)
                    && (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the hovered action row paints its rounded band");
        assert!(
            band.min_x().abs() < 0.01 && (band.width() - 284.0).abs() < 0.01,
            "the band spans the column's content width: {band:?}"
        );
        assert!(
            (band.height() - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "and it is one 32pt row high: {band:?}"
        );
        assert!(icons(&drawn)
            .iter()
            .any(|name| name == SIDEBAR_ICON_NEW_CHAT));
        assert!(texts(&drawn)
            .iter()
            .any(|(text, size)| text == "New conversation"
                && (*size - SIDEBAR_TITLE_SIZE).abs() < 0.01));

        // The glyph starts where a list row's leading slot starts: the card is
        // 3 further out, so its padding is 3 further in.
        let glyph = drawn
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. } if name == SIDEBAR_ICON_NEW_CHAT => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the leading glyph");
        assert!(
            (glyph.x - (SIDEBAR_PAINT_INSET + SIDEBAR_ROW_PADDING_START)).abs() < 0.01,
            "the glyph sits in the list rows' leading column: {glyph:?}"
        );
    }

    /// The hover card is a fixed 244pt box. A long conversation title wraps to
    /// two lines and is ellipsized past that instead of widening the card (the
    /// width cap used to be defeated: the title laid out at its natural width
    /// and pushed the card past its canvas slot).
    #[test]
    fn the_hover_card_never_grows_past_its_244_point_width() {
        let app = app();
        let long_name = "Open-source project for a self-hosted virtual private \
                         server with a very long name that will not fit";
        let mut ui = AgentCardUi::default();
        ui.hover_card_open = true;
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", long_name, "4min")
            .with_status(ConversationStatus::Success)
            .with_hovered(true)
            .with_directory("/Users/adrian.tucicovencogmail.com/Projects/goble/very/deep/path")
            .with_updated_at("2026-11-17T01:00:00Z")
            .with_ui(std::rc::Rc::new(std::cell::RefCell::new(ui)))
            .finish();
        let drawn = render_element(&mut row, vec2f(300.0, 400.0), &app);
        // The card is the raised fill that sits to the right of the row (the
        // row's own hover band is the same token in the dark theme, so the
        // position is what tells them apart).
        let card = drawn
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01
                    && rect.min_x() > 300.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the hover card paints its raised fill");
        assert!(
            (card.width() - HOVER_CARD_WIDTH).abs() < 0.01,
            "the card is its fixed 244 width: {card:?}"
        );
        let border = drawn
            .iter()
            .find_map(|command| match command {
                RenderCommand::StrokeRect {
                    rect,
                    corner_radius,
                    ..
                } if (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01 && rect.min_x() > 300.0 => {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the hover card paints its border");
        assert!(
            border.width() <= HOVER_CARD_WIDTH + 0.01,
            "the card's box never exceeds its width: {border:?}"
        );
        // The card hangs 8pt off the row's painted fill (its layout box ends 3
        // in from the row's own edge), not off the row's box.
        assert!(
            (card.min_x() - (300.0 - SIDEBAR_PAINT_INSET + 8.0)).abs() < 0.5,
            "the card starts 8pt right of the row's painted fill: {card:?}"
        );
        // Two-line wrap with an ellipsis: the title is cut, and the card's Done
        // dot is muted like the web's (not a success badge).
        assert!(
            drawn.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text.ends_with('…')
            )),
            "the long title is ellipsized: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. }
                    if *color == app.theme.color(ColorToken::Success)
            )),
            "the finished state reads muted, not as a success fill"
        );
    }

    /// The hover card's title wraps to two lines like the web's
    /// `numberOfLines={2}`, and only the *second* line is cut with an ellipsis:
    /// a title that fits two lines is shown whole, a longer one keeps two lines
    /// of text before the cut (it used to collapse to one line).
    #[test]
    fn the_hover_card_title_wraps_to_two_lines() {
        let app = app();
        let inner = HOVER_CARD_WIDTH - 2.0 * HOVER_CARD_PADDING;
        let line_height = SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE;
        let lines = |text: &str| {
            crate::platform::text_atlas::wrapped_lines(
                text,
                SIDEBAR_TITLE_SIZE,
                line_height,
                inner,
                FontWeight::Regular,
                FontFamily::System,
                false,
            )
            .len()
        };
        let drawn_title = |title: &str| {
            let mut ui = AgentCardUi::default();
            ui.hover_card_open = true;
            let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", title, "4min")
                .with_status(ConversationStatus::Running)
                .with_hovered(true)
                .with_ui(std::rc::Rc::new(std::cell::RefCell::new(ui)))
                .finish();
            let drawn = render_element(&mut row, vec2f(300.0, 400.0), &app);
            drawn
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText {
                        text,
                        origin,
                        font_size,
                        ..
                    } if origin.x > 300.0 && (*font_size - SIDEBAR_TITLE_SIZE).abs() < 0.01 => {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .expect("the hover card draws its title")
        };

        // A title that fits two lines is drawn whole and occupies both of them.
        let fitting = "Open-source project for a self-hosted VPS";
        let drawn = drawn_title(fitting);
        assert_eq!(drawn, fitting, "a two-line title is shown whole: {drawn:?}");
        assert_eq!(lines(&drawn), 2, "and it wraps onto two lines: {drawn:?}");

        // A longer title keeps two lines of text and cuts the second one with
        // the ellipsis — never a single ellipsized line.
        let long = "Open-source project for a self-hosted virtual private server \
                    with a very long name that will not fit in two lines";
        let drawn = drawn_title(long);
        assert!(
            drawn.ends_with('…'),
            "the cut second line ends in an ellipsis: {drawn:?}"
        );
        assert_eq!(
            lines(&drawn),
            2,
            "the cut keeps two lines of text, not one: {drawn:?}"
        );
        assert!(
            lines(&drawn) > 1,
            "the title is not collapsed to one line: {drawn:?}"
        );
    }

    // ------------------------------------------------------------------
    // The session row's tray/menu + the hover no-resize invariant
    // ------------------------------------------------------------------

    /// A bare row, not boxed, so the private `title_width` can be read.
    fn session_row(time: &str, remote: bool, starred: bool) -> SidebarSessionRow {
        // The app's rows carry the hover strip's quick actions, so the fixtures
        // do too: the strip is then DSH's three controls (More, Archive, Pin).
        SidebarSessionRow::new("c1", "Redesign the sidebar", time)
            .with_remote(remote)
            .with_starred(starred)
            .with_on_quick_pin(|| {})
            .with_on_quick_archive(|| {})
    }

    fn default_group_ui() -> Rc<RefCell<SidebarGroupUi>> {
        Rc::new(RefCell::new(SidebarGroupUi::default()))
    }

    /// One workspace row with the app's tray rows and create control, the shape
    /// `sidebar.rs` builds. The tray state is passed in because the app builds a
    /// fresh row every frame and the row caches its child after its first
    /// layout — so a test that opens the tray re-renders a row built over the
    /// same cell, exactly like the app's next frame.
    fn workspace_row(hovered: bool, ui: Rc<RefCell<SidebarGroupUi>>) -> SidebarGroupRow {
        SidebarGroupRow::new("goble", true)
            .with_hovered(hovered)
            .with_ui(ui)
            .with_viewport_height(1000.0)
            .with_menu_items(vec![
                SidebarMenuItem::new("Rename", "edit", || {}),
                SidebarMenuItem::new("Show in Finder", "folder-open", || {}),
                SidebarMenuItem::new("Delete workspace", "trash-02", || {})
                    .with_separator_before(true)
                    .with_danger(true),
            ])
            .with_on_create_session(|| {})
            .with_on_menu_open(|| {})
    }

    fn lay_out_row(row: &mut dyn Element, width: f32) -> Vector2F {
        row.layout(
            SizeConstraint::loose(vec2f(width, 1000.0)),
            &mut LayoutContext::default(),
            &app(),
        )
    }

    fn lay_out_boxed(row: &mut Box<dyn Element>, width: f32) -> Vector2F {
        row.layout(
            SizeConstraint::loose(vec2f(width, 1000.0)),
            &mut LayoutContext::default(),
            &app(),
        )
    }

    /// The title cell width the spec's formula pins for a row.
    fn expected_title_width(time: &str, remote: bool, starred: bool, row_w: f32) -> f32 {
        (row_w
            - 2.0 * SIDEBAR_PAINT_INSET
            - SIDEBAR_ROW_PADDING_START
            - SIDEBAR_ROW_PADDING_END
            - SIDEBAR_LEADING_SLOT_W
            - SIDEBAR_LABEL_GAP
            - 6.0
            - trailing_slot_width(time, remote, starred))
        .max(0.0)
    }

    /// The rounded card the row paints for a given fill token.
    fn row_band(commands: &[RenderCommand], app: &AppContext, token: ColorToken) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(token)
                && (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
    }

    /// The spec's §3.4 invariant: a session row is 32pt in every state — rest,
    /// hovered, tray open, selected — for every time label and trailing-cluster
    /// combination, and its painted card stays one 32pt band at the 3pt inset.
    #[test]
    fn the_session_row_stays_thirty_two_points_on_hover_and_menu_open() {
        let app = app();
        for (remote, starred) in [(false, false), (true, false), (false, true), (true, true)] {
            for time in ["now", "4min", "17h", "2d", "3mo", "1y"] {
                let mut rest = session_row(time, remote, starred);
                let rest_size = lay_out_row(&mut rest, 284.0);
                assert!(
                    (rest_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
                    "[{time} remote={remote} starred={starred}] resting row is 32: {rest_size:?}"
                );

                let mut hover = session_row(time, remote, starred).with_hovered(true);
                let hover_size = lay_out_row(&mut hover, 284.0);
                assert!(
                    (hover_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
                    "[{time} remote={remote} starred={starred}] hovered row is 32: {hover_size:?}"
                );

                let ui = Rc::new(RefCell::new(AgentCardUi {
                    menu_open: true,
                    ..Default::default()
                }));
                let mut menu = session_row(time, remote, starred).with_ui(ui);
                let menu_size = lay_out_row(&mut menu, 284.0);
                assert!(
                    (menu_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
                    "[{time} remote={remote} starred={starred}] tray-open row is 32: {menu_size:?}"
                );

                // The painted card is one band, inset by 3 and one row high, in
                // each state (rest paints its fill only while selected).
                let mut selected_box: Box<dyn Element> = session_row(time, remote, starred)
                    .with_selected(true)
                    .finish();
                let drawn = render_element(&mut selected_box, vec2f(284.0, 1000.0), &app);
                let band = row_band(&drawn, &app, ColorToken::Selected)
                    .unwrap_or_else(|| panic!("[{time}] the selected row paints its band"));
                assert!(
                    (band.height() - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
                    "{band:?}"
                );
                assert!(
                    (band.min_x() - SIDEBAR_PAINT_INSET).abs() < 0.01,
                    "{band:?}"
                );

                let mut hovered_box: Box<dyn Element> = session_row(time, remote, starred)
                    .with_hovered(true)
                    .finish();
                let drawn = render_element(&mut hovered_box, vec2f(284.0, 1000.0), &app);
                let band = row_band(&drawn, &app, ColorToken::Hover)
                    .unwrap_or_else(|| panic!("[{time}] the hovered row paints its band"));
                assert!(
                    (band.height() - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
                    "{band:?}"
                );
                assert!(
                    (band.min_x() - SIDEBAR_PAINT_INSET).abs() < 0.01,
                    "{band:?}"
                );
            }
        }
    }

    /// The title cell keeps the resting cluster's slot at rest and hands that
    /// room to the hover strip while it is shown — DSH's own row behaviour (its
    /// `rowActions` appear as the `rowResting` cells drop, so the title clips
    /// instead of the row moving). The row stays 32pt and every row below keeps
    /// its place; only which cell owns the trailing room changes.
    #[test]
    fn the_title_cell_gives_its_trailing_room_to_the_hover_strip() {
        for (remote, starred) in [(false, false), (true, false), (false, true), (true, true)] {
            for time in ["now", "4min", "17h", "2d", "3mo", "1y"] {
                let mut rest = session_row(time, remote, starred);
                let rest_size = lay_out_row(&mut rest, 284.0);

                let mut hover = session_row(time, remote, starred).with_hovered(true);
                lay_out_row(&mut hover, 284.0);

                let ui = Rc::new(RefCell::new(AgentCardUi {
                    menu_open: true,
                    ..Default::default()
                }));
                let mut menu = session_row(time, remote, starred).with_ui(ui);
                lay_out_row(&mut menu, 284.0);

                assert!(
                    (rest_size.x - 284.0).abs() < 0.01,
                    "the row takes the sidebar's width: {rest_size:?}"
                );
                assert!(
                    (rest.title_width - expected_title_width(time, remote, starred, 284.0))
                        .abs()
                        < 0.01,
                    "[{time} remote={remote} starred={starred}] the resting slot in the formula: {}",
                    rest.title_width
                );
                // The app's rows carry the Archive and Pin controls too, so the
                // hover strip is three buttons; the tray-open row keeps the same
                // strip (the menu holds the row's hover state, DSH's menuOpen).
                let strip = sidebar_quick_strip_width(3);
                let hover_expected = (284.0
                    - 2.0 * SIDEBAR_PAINT_INSET
                    - SIDEBAR_ROW_PADDING_START
                    - SIDEBAR_ROW_PADDING_END
                    - if false {
                        0.0
                    } else {
                        SIDEBAR_LEADING_SLOT_W + SIDEBAR_LABEL_GAP
                    }
                    - 6.0
                    - strip)
                    .max(0.0);
                assert!(
                    (hover.title_width - hover_expected).abs() < 0.01,
                    "[{time} remote={remote} starred={starred}] the hover strip owns the trailing room: {}",
                    hover.title_width
                );
                assert_eq!(
                    hover.title_width, menu.title_width,
                    "[{time} remote={remote} starred={starred}] the tray keeps the strip shown"
                );
            }
        }
    }

    /// The hover action is still a 24pt hit target even though its layout
    /// footprint shrank to the 16pt glyph: a press 2pt inside the box's trailing
    /// edge (outside the glyph) opens the tray; 2pt outside the box does not.
    #[test]
    fn the_hover_action_keeps_its_twenty_four_point_target() {
        let app = app();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_hovered(true)
            .with_ui(Rc::clone(&ui))
            .finish();
        let size = render_element(&mut row, vec2f(284.0, 1000.0), &app)
            .first()
            .map(|_| ());
        let _ = size;
        let action = rectf(
            284.0 - SIDEBAR_PAINT_INSET - SIDEBAR_ROW_PADDING_END - SIDEBAR_ACTION_BUTTON,
            (SIDEBAR_ROW_HEIGHT - SIDEBAR_ACTION_BUTTON) / 2.0,
            SIDEBAR_ACTION_BUTTON,
            SIDEBAR_ACTION_BUTTON,
        );
        // Inside the 24pt box, outside the 16pt glyph box the row lays out.
        let inside = vec2f(action.max_x() - 2.0, action.min_y() + 11.0);
        let mut event_ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: inside,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: inside,
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut event_ctx, &app);
        }
        assert!(
            ui.borrow().menu_open,
            "the 24pt target extends past the 16pt glyph footprint"
        );

        // Two points further out is outside the target: no tray.
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_hovered(true)
            .with_ui(Rc::clone(&ui))
            .finish();
        let _ = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let outside = vec2f(action.min_x() - 2.0, inside.y);
        for event in [
            DispatchedEvent::MouseDown {
                position: outside,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: outside,
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut event_ctx, &app);
        }
        assert!(
            !ui.borrow().menu_open,
            "a press outside the 24pt target leaves the tray closed"
        );
    }

    /// The tray hangs off the row's new 32pt bottom, not the old 36pt one.
    #[test]
    fn the_tray_opens_at_the_thirty_two_point_row_bottom() {
        let app = app();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            menu_open: true,
            ..Default::default()
        }));
        let mut row = SidebarSessionRow::new("c1", "Work", "4min").with_ui(ui);
        let size = lay_out_row(&mut row, 284.0);
        assert!((size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01);
        row.paint(vec2f(0.0, 10.0), &mut PaintContext::default(), &app);
        let tray = row.tray_bounds().expect("the open tray has bounds");
        assert!(
            (tray.min_y() - (10.0 + SIDEBAR_ROW_HEIGHT + TRAY_GAP)).abs() < 0.01,
            "the tray starts 4pt under the row's 32pt bottom: {tray:?}"
        );
        assert!(
            (tray.width() - SIDEBAR_MENU_MIN_WIDTH).abs() < 0.01,
            "an itemless tray still keeps DSH's 144pt menu minimum: {tray:?}"
        );
    }

    /// The tray's casing is DSH's context-menu card: a 16pt radius, 4pt padding,
    /// a 34pt row with a 12pt band, a 14pt icon and a 13/20 label, and a 0.5pt
    /// separator inset to the row content. Both sidebar trays are built by the
    /// same app-supplied items.
    #[test]
    fn the_sidebar_tray_wears_the_dsh_menu_metrics() {
        let app = app();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            menu_open: true,
            ..Default::default()
        }));
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_hovered(true)
            .with_ui(Rc::clone(&ui))
            .with_menu_items(vec![
                SidebarMenuItem::new("Pin session", SIDEBAR_ICON_PIN_OUTLINE, || {}),
                SidebarMenuItem::new("Copy Session ID", "copy", || {}).with_separator_before(true),
                SidebarMenuItem::new("Delete conversation", "trash-02", || {}).with_danger(true),
            ])
            .finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);

        // The casing: the raised surface with a 16pt radius.
        let casing = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - SIDEBAR_MENU_RADIUS).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the tray paints DSH's 16pt-radius card");
        assert!(
            (casing.width()
                - sidebar_menu_width(&[
                    SidebarMenuItem::new("Pin session", SIDEBAR_ICON_PIN_OUTLINE, || {}),
                    SidebarMenuItem::new("Copy Session ID", "copy", || {})
                        .with_separator_before(true),
                    SidebarMenuItem::new("Delete conversation", "trash-02", || {})
                        .with_danger(true),
                ]))
            .abs()
                < 0.01,
            "the card takes its contents' width between DSH's bounds: {casing:?}"
        );

        // The rows: 34pt tall, a 13pt label, a 12pt band radius, the visible
        // hover fill and a 14pt icon.
        let label = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    font_size,
                    origin,
                    ..
                } if text == "Copy Session ID" => Some((*font_size, *origin)),
                _ => None,
            })
            .expect("the tray draws its row labels");
        assert!(
            (label.0 - SIDEBAR_MENU_LABEL_SIZE).abs() < 0.01,
            "the row label is DSH's 13pt menu type: {label:?}"
        );
        let bands: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    corner_radius,
                    ..
                } if (*corner_radius - SIDEBAR_MENU_ROW_RADIUS).abs() < 0.01
                    && rect.min_y() > SIDEBAR_ROW_HEIGHT =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        assert!(
            !bands.is_empty(),
            "a row paints its 12pt-radius band: {commends:?}",
            commends = commands
        );
        for band in &bands {
            assert!(
                (band.height() - SIDEBAR_MENU_ROW_HEIGHT).abs() < 0.01,
                "a row is DSH's 34pt tall: {band:?}"
            );
        }
        let icon = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, size, .. } if name == "copy" => Some(*size),
                _ => None,
            })
            .expect("the row draws its icon");
        assert!(
            (icon - SIDEBAR_MENU_ICON_SIZE).abs() < 0.01,
            "the row icon is DSH's 14pt: {icon}"
        );

        // The separator: a 0.5pt rule inset 8pt inside the row content's width.
        let rule = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Border)
                        && (rect.height() - SIDEBAR_MENU_SEPARATOR_HEIGHT).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the separator draws DSH's 0.5pt rule");
        assert!(
            (rule.min_x() - (casing.min_x() + SIDEBAR_MENU_PADDING + SIDEBAR_MENU_SEPARATOR_INSET))
                .abs()
                < 0.01,
            "the rule is inset to the row content: {rule:?} in {casing:?}"
        );
        assert!(
            (rule.width()
                - (casing.width()
                    - 2.0 * SIDEBAR_MENU_PADDING
                    - 2.0 * SIDEBAR_MENU_SEPARATOR_INSET))
                .abs()
                < 0.01,
            "the rule spans the inset content width: {rule:?}"
        );

        // The destructive row is the error colour, its label too.
        let danger_label = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } if text == "Delete conversation" => {
                    Some(*color)
                }
                _ => None,
            })
            .expect("the destructive row is drawn");
        assert_eq!(
            danger_label,
            app.theme.color(ColorToken::Error),
            "the Delete row reads in the error ink"
        );
    }

    /// The group row is 32pt rested and hovered (its folder→triangle swap keeps
    /// a 16pt leading slot either way).
    #[test]
    fn a_group_row_stays_thirty_two_points_on_hover() {
        let mut rest: Box<dyn Element> = SidebarGroupRow::new("goble", false).finish();
        let rest_size = lay_out_boxed(&mut rest, 284.0);
        assert!(
            (rest_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "{rest_size:?}"
        );
        let mut hover: Box<dyn Element> = SidebarGroupRow::new("goble", false)
            .with_hovered(true)
            .finish();
        let hover_size = lay_out_boxed(&mut hover, 284.0);
        assert!(
            (hover_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "{hover_size:?}"
        );
    }

    /// The "New conversation" action row is 32pt rested and hovered.
    #[test]
    fn a_sidebar_action_row_stays_thirty_two_points_on_hover() {
        let mut rest: Box<dyn Element> =
            SidebarActionRow::new(SIDEBAR_ICON_NEW_CHAT, "New conversation").finish();
        let rest_size = lay_out_boxed(&mut rest, 284.0);
        assert!(
            (rest_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "{rest_size:?}"
        );
        let mut hover: Box<dyn Element> =
            SidebarActionRow::new(SIDEBAR_ICON_NEW_CHAT, "New conversation")
                .with_hovered(true)
                .finish();
        let hover_size = lay_out_boxed(&mut hover, 284.0);
        assert!(
            (hover_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "{hover_size:?}"
        );
    }

    /// A row near the window's bottom flips its tall tray above itself (and the
    /// tray's hit area follows) instead of opening under the window's edge.
    #[test]
    fn the_tray_flips_above_a_row_near_the_window_bottom() {
        let app = app();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            menu_open: true,
            ..Default::default()
        }));
        let mut items = Vec::new();
        for label in [
            "Pin session",
            "Rename",
            "Fork session",
            "Copy Session ID",
            "Archive session",
            "Delete conversation",
        ] {
            items.push(SidebarMenuItem::new(label, SIDEBAR_ICON_PIN_OUTLINE, || {}));
        }
        let mut row = SidebarSessionRow::new("c1", "Work", "4min")
            .with_ui(ui)
            .with_menu_items(items)
            .with_viewport_height(600.0);
        lay_out_row(&mut row, 284.0);
        row.paint(vec2f(0.0, 560.0), &mut PaintContext::default(), &app);
        let tray = row.tray_bounds().expect("the open tray has bounds");
        assert!(
            tray.max_y() <= 600.0,
            "the flipped tray stays inside the window: {tray:?}"
        );
        assert!(
            tray.max_y() <= 560.0,
            "the flipped tray sits above the row: {tray:?}"
        );
        assert!(tray.min_y() >= 0.0, "and not off the top: {tray:?}");
        let at = vec2f(
            tray.min_x() + tray.width() / 2.0,
            tray.min_y() + tray.height() / 2.0,
        );
        assert!(
            row.pointer_on_tray(at),
            "the hit area follows the flipped tray"
        );
    }

    /// An archived row keeps the normal 32pt metrics in both states. The app
    /// hides it from the default list, but the row itself still lays out and
    /// paints like any other.
    #[test]
    fn an_archived_row_keeps_its_metrics() {
        let mut rest = SidebarSessionRow::new("c1", "Work", "4min").with_archived(true);
        let rest_size = lay_out_row(&mut rest, 284.0);
        assert!(
            (rest_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "{rest_size:?}"
        );
        assert!((rest_size.x - 284.0).abs() < 0.01, "{rest_size:?}");
        let mut hover = SidebarSessionRow::new("c1", "Work", "4min")
            .with_archived(true)
            .with_hovered(true);
        let hover_size = lay_out_row(&mut hover, 284.0);
        assert!(
            (hover_size.y - SIDEBAR_ROW_HEIGHT).abs() < 0.01,
            "{hover_size:?}"
        );
    }

    /// An archived row drops the live status glyph, the resting pin and the
    /// normal title ink: it reads as history, not as a running conversation.
    #[test]
    fn an_archived_row_paints_no_status_and_no_pin() {
        let app = app();
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_archived(true)
            .with_starred(true)
            .with_status(ConversationStatus::Running)
            .finish();
        let drawn = render_element(&mut row, vec2f(284.0, 32.0), &app);
        assert!(
            !icons(&drawn)
                .iter()
                .any(|name| name == SIDEBAR_ICON_STATUS_RUNNING),
            "no running ring in the leading slot: {:?}",
            icons(&drawn)
        );
        assert!(
            !icons(&drawn)
                .iter()
                .any(|name| name == SIDEBAR_ICON_PIN_FILL),
            "no resting pin on an archived row: {:?}",
            icons(&drawn)
        );
        let title_color = drawn.iter().find_map(|command| match command {
            RenderCommand::DrawText {
                text,
                color,
                font_size,
                ..
            } if text == "Work" && (*font_size - SIDEBAR_TITLE_SIZE).abs() < 0.01 => Some(*color),
            _ => None,
        });
        assert_eq!(
            title_color,
            Some(app.theme.color(ColorToken::Muted)),
            "the archived title reads muted"
        );
    }

    /// The hover card reports the archived state on its own line (and keeps its
    /// fixed 244pt width).
    #[test]
    fn the_hover_card_reports_archived() {
        let app = app();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            hover_card_open: true,
            ..Default::default()
        }));
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_archived(true)
            .with_hovered(true)
            .with_ui(ui)
            .finish();
        let drawn = render_element(&mut row, vec2f(300.0, 400.0), &app);
        let archived = drawn.iter().any(|command| {
            matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Archived" && origin.x > 300.0
            )
        });
        assert!(
            archived,
            "the hover card carries the Archived line: {drawn:?}"
        );
        let card = drawn
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - SIDEBAR_ROW_RADIUS).abs() < 0.01
                    && rect.min_x() > 300.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the hover card paints its raised fill");
        assert!((card.width() - 244.0).abs() < 0.01, "{card:?}");
    }

    /// Six app-supplied rows (with three group separators) draw in order; the
    /// keyboard walks only the actionable rows, Enter runs the highlighted one
    /// exactly once, and Escape closes without running anything.
    #[test]
    fn the_session_tray_lists_the_app_supplied_rows_in_order() {
        let app = app();

        let build = || {
            let labels = [
                ("Pin session", SIDEBAR_ICON_PIN_OUTLINE, false, false),
                ("Rename", "edit", false, false),
                ("Fork session", "branch", false, false),
                ("Copy Session ID", "copy", true, false),
                ("Archive session", "archive", true, false),
                ("Delete conversation", "trash-02", true, true),
            ];
            let counters: Vec<Rc<RefCell<usize>>> = (0..labels.len())
                .map(|_| Rc::new(RefCell::new(0usize)))
                .collect();
            let items: Vec<SidebarMenuItem> = labels
                .iter()
                .zip(counters.iter())
                .map(|((label, icon, separator, danger), counter)| {
                    let counter = Rc::clone(counter);
                    SidebarMenuItem::new(*label, icon, move || *counter.borrow_mut() += 1)
                        .with_separator_before(*separator)
                        .with_danger(*danger)
                })
                .collect();
            let ui = Rc::new(RefCell::new(AgentCardUi {
                menu_open: true,
                ..Default::default()
            }));
            let row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
                .with_hovered(true)
                .with_ui(Rc::clone(&ui))
                .with_menu_items(items)
                .finish();
            (row, ui, counters)
        };

        let (mut row, _ui, counters) = build();
        let drawn = render_element(&mut row, vec2f(284.0, 500.0), &app);
        let tray_labels: Vec<String> = drawn
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    origin,
                    font_size,
                    ..
                } if (*font_size - SIDEBAR_MENU_LABEL_SIZE).abs() < 0.01
                    && origin.y > SIDEBAR_ROW_HEIGHT =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            tray_labels,
            vec![
                "Pin session",
                "Rename",
                "Fork session",
                "Copy Session ID",
                "Archive session",
                "Delete conversation",
            ],
            "the tray draws the app's rows in order"
        );
        let separators = drawn
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, color, .. }
                        if *color == app.theme.color(ColorToken::Border)
                            && (rect.height() - SIDEBAR_MENU_SEPARATOR_HEIGHT).abs() < 0.01
                )
            })
            .count();
        assert_eq!(separators, 3, "one hairline per separator_before row");

        // ArrowDown walks the rows; Enter runs the highlighted one exactly once
        // and puts the tray away.
        for target in 0..counters.len() {
            let (mut row, ui, counters) = build();
            let _ = render_element(&mut row, vec2f(284.0, 500.0), &app);
            let mut event_ctx = EventContext::default();
            for _ in 0..target {
                assert!(row.dispatch_event(&key_event("ArrowDown"), &mut event_ctx, &app));
            }
            assert_eq!(
                ui.borrow().highlight,
                target,
                "the highlight walks actionable rows only"
            );
            assert!(row.dispatch_event(&key_event("Enter"), &mut event_ctx, &app));
            assert_eq!(
                *counters[target].borrow(),
                1,
                "Enter runs the highlighted row once"
            );
            assert!(!ui.borrow().menu_open, "and closes the tray");
            for (index, counter) in counters.iter().enumerate() {
                if index != target {
                    assert_eq!(*counter.borrow(), 0, "no other row ran");
                }
            }
        }

        // Escape closes the tray and runs nothing.
        let (mut row, ui, counters) = build();
        let _ = render_element(&mut row, vec2f(284.0, 500.0), &app);
        let mut event_ctx = EventContext::default();
        row.dispatch_event(&key_event("ArrowDown"), &mut event_ctx, &app);
        row.dispatch_event(&key_event("ArrowDown"), &mut event_ctx, &app);
        assert!(row.dispatch_event(&key_event("Escape"), &mut event_ctx, &app));
        assert!(!ui.borrow().menu_open);
        assert!(counters.iter().all(|counter| *counter.borrow() == 0));
    }

    fn key_event(key: &str) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: key.to_string(),
            modifiers: crate::event::ModifiersState::none(),
        }
    }

    /// One workspace row's hover strip is DSH's `ProjectRow` pair: the
    /// context-menu trigger and the New-session control, in that order, and the
    /// create control runs the app's callback without opening the tray.
    #[test]
    fn a_workspace_row_draws_its_hover_strip_and_runs_its_controls() {
        let app = app();
        let mut row: Box<dyn Element> = workspace_row(true, default_group_ui()).finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let drawn: Vec<String> = icons(&commands);
        assert!(
            drawn.iter().any(|name| name == SIDEBAR_ICON_ELLIPSIS),
            "the row draws its context-menu trigger: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|name| name == SIDEBAR_ICON_NEW_CHAT),
            "the row draws its New-session control: {drawn:?}"
        );
        // DSH's order: the trigger first, the create control last.
        let origin = |name: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawIcon {
                        name: drawn,
                        origin,
                        ..
                    } if drawn == name => Some(*origin),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the row draws {name:?}: {commands:?}"))
        };
        assert!(
            origin(SIDEBAR_ICON_ELLIPSIS).x < origin(SIDEBAR_ICON_NEW_CHAT).x,
            "the menu trigger leads the create control"
        );

        // A press on the New-session control runs its action and leaves the
        // tray closed.
        let created = Rc::new(RefCell::new(false));
        let flag = Rc::clone(&created);
        let mut row: Box<dyn Element> = SidebarGroupRow::new("goble", true)
            .with_hovered(true)
            .with_ui(Rc::new(RefCell::new(SidebarGroupUi::default())))
            .with_menu_items(vec![SidebarMenuItem::new("Rename", "edit", || {})])
            .with_on_create_session(move || *flag.borrow_mut() = true)
            .finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let new_chat = origin(SIDEBAR_ICON_NEW_CHAT);
        let at = new_chat + vec2f(8.0, 8.0);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(
            *created.borrow(),
            "the create control runs the app's action"
        );
    }

    /// The workspace tray is the app-supplied list (Rename, Show in Finder, then
    /// Delete behind a separator); the dots toggle it, its rows run, and the row keeps the
    /// strip on screen while the menu is up.
    #[test]
    fn a_workspace_row_opens_its_tray_and_keeps_the_strip_shown() {
        let app = app();
        let ui = default_group_ui();
        let mut row: Box<dyn Element> = workspace_row(true, Rc::clone(&ui)).finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let dots = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. } if name == SIDEBAR_ICON_ELLIPSIS => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the hovered row draws its dots");
        let at = dots + vec2f(8.0, 8.0);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(ui.borrow().menu_open, "the dots open the workspace tray");
        // The app rebuilds the row every frame: build the frame the next one
        // would, over the same tray cell.
        let mut row: Box<dyn Element> = workspace_row(true, Rc::clone(&ui)).finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let labels = texts(&commands);
        assert!(
            labels
                .iter()
                .any(|(text, size)| text == "Rename"
                    && (*size - SIDEBAR_MENU_LABEL_SIZE).abs() < 0.01),
            "the tray lists its rows: {labels:?}"
        );
        assert!(
            labels.iter().any(|(text, _)| text == "Show in Finder"),
            "the tray lists the reveal row: {labels:?}"
        );
        assert!(
            labels.iter().any(|(text, _)| text == "Delete workspace"),
            "the tray lists the destructive row: {labels:?}"
        );

        // The tray-open row keeps its strip: DSH's `menuOpen` keeps the
        // affordances up, so the dots do not vanish from under the pointer.
        let open = render_element(
            &mut workspace_row(
                true,
                Rc::new(RefCell::new(SidebarGroupUi {
                    menu_open: true,
                    highlight: 0,
                })),
            )
            .finish(),
            vec2f(284.0, 1000.0),
            &app,
        );
        assert!(
            icons(&open)
                .iter()
                .any(|name| name == SIDEBAR_ICON_ELLIPSIS),
            "the open row still draws its strip"
        );
    }

    /// DSH opens a context menu unfilled: the first row is focused but paints
    /// no fill until the pointer or the keyboard reaches it. The sidebar trays
    /// follow that, and the first arrow key enters the row walk at row 0.
    #[test]
    fn a_sidebar_tray_opens_unfilled_and_the_first_arrow_enters_the_walk() {
        let app = app();
        let ui = default_group_ui();
        let mut row: Box<dyn Element> = workspace_row(true, Rc::clone(&ui)).finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let dots = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. } if name == SIDEBAR_ICON_ELLIPSIS => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the hovered row draws its dots");
        let at = dots + vec2f(8.0, 8.0);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(ui.borrow().menu_open, "the dots open the tray");
        assert_eq!(
            ui.borrow().highlight,
            NO_HIGHLIGHT,
            "opening paints no row: DSH's menu opens unfilled"
        );

        // The frame the next one would build: no selection band below the row.
        let frame = || {
            let mut row: Box<dyn Element> = workspace_row(true, Rc::clone(&ui)).finish();
            render_element(&mut row, vec2f(284.0, 1000.0), &app)
        };
        let bands = |commands: &[RenderCommand]| -> usize {
            commands
                .iter()
                .filter(|command| {
                    matches!(
                        command,
                        RenderCommand::FillRect { rect, color, corner_radius }
                            if *color == app.theme.color(ColorToken::Selected)
                                && (*corner_radius - SIDEBAR_MENU_ROW_RADIUS).abs() < 0.01
                                && rect.min_y() > SIDEBAR_ROW_HEIGHT
                    )
                })
                .count()
        };
        let unfilled = frame();
        assert_eq!(bands(&unfilled), 0, "no row is filled on open");

        // The first ArrowDown enters the walk at the first row.
        let mut row: Box<dyn Element> = workspace_row(true, Rc::clone(&ui)).finish();
        let _ = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        assert!(row.dispatch_event(&key_event("ArrowDown"), &mut ctx, &app));
        assert_eq!(ui.borrow().highlight, 0, "Down enters at the first row");
        assert_eq!(bands(&frame()), 1, "and the first row paints its fill");
    }

    /// Enter on a freshly opened tray runs its first row even though no row is
    /// painted: DSH focuses the first item on open, so keyboard activation does
    /// not need an arrow first.
    #[test]
    fn enter_on_a_freshly_opened_tray_runs_the_first_row() {
        let app = app();
        let ui = default_group_ui();
        let runs = Rc::new(RefCell::new(0usize));
        let counter = Rc::clone(&runs);
        let items = vec![
            SidebarMenuItem::new("Rename", "edit", move || *counter.borrow_mut() += 1),
            SidebarMenuItem::new("Delete workspace", "trash-02", || {})
                .with_separator_before(true)
                .with_danger(true),
        ];
        let mut row: Box<dyn Element> = SidebarGroupRow::new("goble", true)
            .with_hovered(true)
            .with_ui(Rc::clone(&ui))
            .with_viewport_height(1000.0)
            .with_menu_items(items.clone())
            .with_on_create_session(|| {})
            .with_on_menu_open(|| {})
            .finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let dots = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. } if name == SIDEBAR_ICON_ELLIPSIS => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the hovered row draws its dots");
        let at = dots + vec2f(8.0, 8.0);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(ui.borrow().menu_open);
        let mut next: Box<dyn Element> = SidebarGroupRow::new("goble", true)
            .with_hovered(true)
            .with_ui(Rc::clone(&ui))
            .with_viewport_height(1000.0)
            .with_menu_items(items)
            .with_on_create_session(|| {})
            .with_on_menu_open(|| {})
            .finish();
        let _ = render_element(&mut next, vec2f(284.0, 1000.0), &app);
        assert!(next.dispatch_event(&key_event("Enter"), &mut ctx, &app));
        assert_eq!(*runs.borrow(), 1, "Enter runs the first row straight away");
        assert!(!ui.borrow().menu_open, "and closes the tray");
    }

    /// A conversation row's hover strip is DSH's three controls in order — the
    /// tray trigger, Archive, Pin — and both quick actions run their callbacks
    /// without opening the tray.
    #[test]
    fn a_session_row_draws_archive_and_pin_and_runs_them() {
        let app = app();
        let mut row: Box<dyn Element> = session_row("4min", false, false)
            .with_hovered(true)
            .finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let drawn: Vec<String> = icons(&commands);
        let position = |name: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawIcon {
                        name: drawn,
                        origin,
                        ..
                    } if drawn == name => Some(*origin),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the row draws {name:?}: {commands:?}"))
        };
        for icon in [
            SIDEBAR_ICON_ELLIPSIS,
            SIDEBAR_ICON_ARCHIVE,
            SIDEBAR_ICON_PIN_OUTLINE,
        ] {
            assert!(
                drawn.iter().any(|name| name == icon),
                "the strip draws {icon:?}"
            );
        }
        assert!(position(SIDEBAR_ICON_ELLIPSIS).x < position(SIDEBAR_ICON_ARCHIVE).x);
        assert!(position(SIDEBAR_ICON_ARCHIVE).x < position(SIDEBAR_ICON_PIN_OUTLINE).x);

        let archived = Rc::new(RefCell::new(false));
        let pinned = Rc::new(RefCell::new(false));
        let archive_flag = Rc::clone(&archived);
        let pin_flag = Rc::clone(&pinned);
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_hovered(true)
            .with_on_quick_archive(move || *archive_flag.borrow_mut() = true)
            .with_on_quick_pin(move || *pin_flag.borrow_mut() = true)
            .finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let pin = position(SIDEBAR_ICON_PIN_OUTLINE);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: pin + vec2f(8.0, 8.0),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: pin + vec2f(8.0, 8.0),
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(*pinned.borrow(), "the Pin control runs its action");

        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let archive = position(SIDEBAR_ICON_ARCHIVE);
        for event in [
            DispatchedEvent::MouseDown {
                position: archive + vec2f(8.0, 8.0),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: archive + vec2f(8.0, 8.0),
                button: 0,
            },
        ] {
            row.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(*archived.borrow(), "the Archive control runs its action");
        let _ = commands;
    }

    /// An archived conversation row offers the restore instead of Archive and
    /// no Pin at all (DSH resolves the same list for an archived session).
    #[test]
    fn an_archived_session_row_offers_only_the_restore() {
        let app = app();
        let mut row: Box<dyn Element> = SidebarSessionRow::new("c1", "Work", "4min")
            .with_archived(true)
            .with_hovered(true)
            .with_on_quick_archive(|| {})
            .with_on_quick_pin(|| {})
            .finish();
        let commands = render_element(&mut row, vec2f(284.0, 1000.0), &app);
        let drawn: Vec<String> = icons(&commands);
        assert!(drawn.iter().any(|name| name == SIDEBAR_ICON_UNARCHIVE));
        assert!(!drawn.iter().any(|name| name == SIDEBAR_ICON_ARCHIVE));
        assert!(!drawn.iter().any(|name| name == SIDEBAR_ICON_PIN_OUTLINE));
        assert!(!drawn.iter().any(|name| name == SIDEBAR_ICON_PIN_FILL));
    }
}
