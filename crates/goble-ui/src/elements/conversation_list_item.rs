use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Empty, EventContext, Expanded,
    Fill, Flex, HoverRow, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point,
    SizeConstraint, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

/// Extra height above and below a card's rows. The card is a full-width band,
/// so without it the two text rows sit flush against the cards above and below
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

/// The status square the card leads with. It is a marker, not a badge: half
/// the linear space of the environment avatar it replaces, so the subject's
/// text stays the card's widest element.
const STATUS_SIZE: f32 = 12.0;

/// The widest the card's tray grows before its rows' own width is what it takes.
/// Wide enough for "Delete conversation" on one line with the row's own
/// padding, since the tray now opens beside the dots where that width is free.
const TRAY_MAX_WIDTH: f32 = 200.0;

/// How far the tray's top sits below the card the control is on.
const TRAY_GAP: f32 = 4.0;

/// The tray's own corner radius, inset and hairline: the card is the same one
/// the rich input's controls open beside themselves, so the two trays read as
/// one surface (see `PopupMenu`).
const TRAY_RADIUS: f32 = 6.0;
/// The tray's vertical padding, and the horizontal inset each of its rows keeps
/// for its own content. The tray itself has no horizontal padding: a row's band
/// spans the tray edge to edge.
const TRAY_PADDING: f32 = 8.0;

/// The tray's rows, in paint order: the star toggle first, then delete.
const STAR_ROW: usize = 0;
const DELETE_ROW: usize = 1;

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
}

impl AgentCardUi {
    /// Put the tray away with its highlight back on the first row, where the
    /// next open starts.
    fn close_menu(&mut self) {
        self.menu_open = false;
        self.highlight = STAR_ROW;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConversationStatus {
    #[default]
    Default,
    /// A turn is running, or the agent is waiting on the user (an approval or
    /// a question).
    Waiting,
    Success,
    Error,
    Stopped,
}

impl ConversationStatus {
    /// The colour the card's status square carries: gray for the quiet states,
    /// yellow while the agent is working or waiting, red for a failed turn and
    /// green for one that finished.
    pub fn color_token(self) -> ColorToken {
        match self {
            Self::Waiting => ColorToken::Warning,
            Self::Success => ColorToken::Success,
            Self::Error => ColorToken::Error,
            Self::Stopped | Self::Default => ColorToken::Muted,
        }
    }
}

pub struct ConversationListItem {
    id: String,
    name: String,
    last_response: String,
    timestamp: String,
    /// The directory the conversation works in, drawn under its subject. `None`
    /// (or empty) draws no row, so a conversation that has not run anywhere yet
    /// keeps the two-line card.
    directory: Option<String>,
    selected: bool,
    /// Whether the conversation is starred, which the card marks and the
    /// sidebar's Starred section lists.
    starred: bool,
    /// What the card's status square reports. [`ConversationStatus::Default`]
    /// and [`ConversationStatus::Stopped`] draw the neutral gray square.
    status: ConversationStatus,
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
        last_response: impl Into<String>,
        timestamp: impl Into<String>,
        ui: Rc<RefCell<AgentCardUi>>,
        selected: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            last_response: last_response.into(),
            timestamp: timestamp.into(),
            directory: None,
            selected,
            starred: false,
            status: ConversationStatus::Default,
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

    /// Set the conversation's work environment. The card no longer draws it —
    /// the environment is chosen in the topbar and only filters the list — so
    /// the value is accepted for callers that still carry it and not stored.
    pub fn with_workspace_routing(self, _routing: impl Into<String>) -> Self {
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
        let spacing = 10.0_f32;
        let xs = 6.0_f32;

        let ui = self.ui.borrow();
        let hover = ui.hover;
        let menu_open = ui.menu_open;
        let highlight = self.menu_row(ui.highlight);

        // The band's own colour, which `paint` fills the whole card with (and
        // the fade blends into): the pointer over the card, the selected
        // conversation, or the surface the card sits on.
        self.bg = if self.selected {
            app.theme.color(ColorToken::Selected)
        } else if hover {
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

        let last_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Text::new(self.last_response.clone())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish();

        let mut text_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(name_row);
        // The directory the conversation works in, directly under its subject:
        // it says where the work happens, which the title alone cannot.
        if let Some(directory) = self.directory.clone() {
            text_column = text_column.with_child(
                Flex::row()
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
                    )
                    .finish(),
            );
        }
        let text_column = text_column.with_child(last_row).finish();

        // Status square: the conversation's last turn reads at a glance —
        // gray when there is nothing to report, yellow while the agent is
        // working or waiting on the user, red when the last turn failed and
        // green when it finished. Square and unbacked, like the rest of the
        // sidebar's marks; the environment avatar it replaces is gone.
        let status = Container::new(
            Empty::new()
                .with_size(vec2f(STATUS_SIZE, STATUS_SIZE))
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(self.status.color_token())))
        .finish();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(status)
            .with_child(Expanded::new(text_column).finish());

        // Square: the sidebar is a flat surface, so a card is a full-width band
        // rather than a rounded widget. The band's fill is painted by `paint` and
        // not by this container: the row is clipped at the control's gutter while
        // the band is not, so the two cannot come from one clipped subtree.
        self.row = Some(
            Container::new(row.finish())
                .with_padding(crate::style::EdgeInsets::new(
                    0.0,
                    CARD_EXTRA_HEIGHT,
                    0.0,
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

    /// The area the row may paint in: the card's width less the control's
    /// gutter, and the row's own band only, so the menu below it is not clipped
    /// with the row.
    fn row_clip(&self, origin: Vector2F, size: Vector2F) -> RectF {
        rectf(
            origin.x,
            origin.y,
            (size.x - self.dots_inset - DOTS_SIZE - DOTS_CLEARANCE).max(0.0),
            self.row_height,
        )
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

/// The inset a tray row gives its content. The tray has no horizontal padding
/// of its own, so the band a row paints spans the tray edge to edge; the label
/// keeps the inset the ghost button gave it — the tray's padding plus the
/// control's own horizontal padding.
fn menu_row_padding(app: &AppContext) -> EdgeInsets {
    let horizontal = TRAY_PADDING + app.theme.spacing_px(SpacingToken::Md);
    let vertical = app.theme.spacing_px(SpacingToken::Sm);
    EdgeInsets::new(horizontal, vertical, horizontal, vertical)
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
            let menu_size = menu
                .borrow_mut()
                .layout(SizeConstraint::loose(vec2f(TRAY_MAX_WIDTH, 400.0)), ctx, app);
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
        // long one-line last message is cut at the card's right edge instead of
        // spilling into the main area.
        let card = rectf(origin.x, origin.y, size.x, size.y);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(card, self.bg, 0.0);
            renderer.clip_rect(card);
        }

        // The row stops short of the control, so the title is cut before the
        // dots rather than painted under them. The menu and the control are
        // painted past that clip — inside the card's own, which keeps them
        // within the surface they belong to.
        let clipped_row = self.dots.is_some();
        if clipped_row {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.clip_rect(self.row_clip(origin, size));
            }
        }
        if let Some(row) = self.row.as_mut() {
            row.paint(origin, ctx, app);
        }
        if clipped_row {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.pop_clip();
            }
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

        // Fade the right edge of the row until the 3-dot menu appears (the row
        // is then clipped clear of the dots). Skip while the menu is open so the
        // fade never covers the delete button.
        let ui = self.ui.borrow();
        if !ui.hover && !ui.menu_open {
            let fade_w = 24.0_f32;
            if size.x > fade_w {
                let rect = rectf(origin.x + size.x - fade_w, origin.y, fade_w, size.y);
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
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "I finished the task.",
            "40 min ago",
            ui,
            false,
        );
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
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "Hello",
            "5 min ago",
            ui,
            false,
        )
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
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "Hello",
            "5 min ago",
            Rc::clone(&ui),
            false,
        );
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
            "Ada",
            "A very long last message that should fade out at the right edge",
            "5 min ago",
            ui,
            false,
        );
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
        let counts = crate::test_util::command_counts(&commands);
        assert_eq!(counts.fill_rect_fade, 1, "idle card should emit a right fade");
        assert!(counts.draw_text > 0, "card should render text");
    }

    /// The 3-dot control's box and colour, from the icon it paints.
    fn dots_box(commands: &[RenderCommand]) -> Option<RectF> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name,
                origin,
                size,
                ..
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
            "A long last message",
            "5 min ago",
            Rc::new(RefCell::new(AgentCardUi::default())),
            false,
        );
        let mut element: Box<dyn Element> = Box::new(idle);
        let width = 200.0;
        let commands = crate::test_util::render_element(&mut element, vec2f(width, 200.0), &app);
        assert!(dots_box(&commands).is_none(), "an idle card draws no dots");
        assert!(
            commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::ClipRect(rect) => Some(rect.max_x()),
                    _ => None,
                })
                .all(|max_x| max_x >= width - 0.01),
            "and needs no gutter clip: the card's own clip is enough"
        );

        // The hovered card pins its control to its trailing edge, inset by the
        // gap it was given.
        let hovered = ConversationListItem::new(
            "c1",
            long_title,
            "A long last message",
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
        assert_eq!(dots.width(), DOTS_SIZE, "the control's box is the glyph's box");
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
            "A long last message",
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
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "A long last message",
            "5 min ago",
            ui,
            false,
        )
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
        let has_star = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text == "Star")
        });
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
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "A long last message",
            "5 min ago",
            Rc::clone(&ui),
            false,
        )
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
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "A long last message",
            "5 min ago",
            Rc::clone(&ui),
            false,
        )
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

    /// The status square the card paints: its box and its fill.
    fn status_square(commands: &[RenderCommand]) -> Option<(RectF, crate::color::ColorU)> {
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if rect.width() == STATUS_SIZE && rect.height() == STATUS_SIZE =>
            {
                Some((*rect, *color))
            }
            _ => None,
        })
    }

    /// Every conversation's card leads with a 12x12 filled square, and its
    /// colour is the status: gray when idle or stopped, yellow while the agent
    /// works or waits, red for a failed turn and green for a finished one. The
    /// environment icon the card used to draw is gone.
    #[test]
    fn the_card_draws_a_status_square_in_its_status_colour() {
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
                "I finished the task.",
                "40 min ago",
                Rc::new(RefCell::new(AgentCardUi::default())),
                false,
            )
            .with_status(status);
            let mut element: Box<dyn Element> = Box::new(item);
            let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

            let (square, color) =
                status_square(&commands).unwrap_or_else(|| panic!("{status:?}: no square"));
            assert_eq!((square.width(), square.height()), (STATUS_SIZE, STATUS_SIZE));
            assert_eq!(color, app.theme.color(token), "{status:?} colour");
            assert!(square.min_x() < 1.0, "{status:?}: the square leads the card");
            assert!(
                !commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::DrawIcon { name, .. } if name.starts_with("conversation-")
                )),
                "{status:?}: the environment icon is not drawn any more"
            );
        }
    }
}
