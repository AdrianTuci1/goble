use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Expanded, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize,
    PaintContext, Point, SizeConstraint, Text, TopbarButton,
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

/// The widest the card's tray grows before its rows' own width is what it takes.
const TRAY_MAX_WIDTH: f32 = 168.0;

/// How far the tray's top sits below the card the control is on.
const TRAY_GAP: f32 = 4.0;

/// The tray's own corner radius, inset and hairline: the card is the same one
/// the rich input's controls open beside themselves, so the two trays read as
/// one surface (see `PopupMenu`).
const TRAY_RADIUS: f32 = 6.0;
const TRAY_PADDING: f32 = 8.0;

/// Per-card interaction state that must survive the per-frame element rebuild.
/// Owned by the app (a map keyed by conversation id) and shared with the card
/// through `Rc<RefCell<_>>`, so hover / the delete menu persist across frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct AgentCardUi {
    pub hover: bool,
    pub menu_open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConversationStatus {
    #[default]
    Default,
    Success,
    Error,
    Stopped,
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
    /// The work environment this conversation runs in (`"local"` / `"remote"`),
    /// used to pick the environment SVG shown in the card.
    workspace_routing: String,
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
    /// The row: the avatar and the conversation's own text, painted clipped
    /// clear of the control.
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
            workspace_routing: "local".to_string(),
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

    /// Set the conversation's work environment, which picks the SVG shown in
    /// the card (local vs remote avatar).
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
        let spacing = 10.0_f32;
        let xs = 6.0_f32;

        let ui = self.ui.borrow();
        let hover = ui.hover;
        let menu_open = ui.menu_open;

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

        // Environment SVG badge: the conversation's work medium (Local or
        // Remote) is shown directly on the card, mirroring the topbar medium
        // selector so it is obvious where the conversation runs.
        let icon_name = if self.workspace_routing == "remote" {
            "conversation-remote"
        } else {
            "conversation-local"
        };
        let avatar = Container::new(
            Icon::new(icon_name)
                .with_size(16.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(crate::style::EdgeInsets::new(5.0, 5.0, 5.0, 5.0))
        .finish();

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(avatar)
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
        // anchored to the 3-dot control rather than in rows under the row — the
        // buttons open their own trays beside themselves the same way. It is
        // laid out here and placed by `paint`, so it takes no room in the list
        // and opening it does not push the cards below it down.
        if menu_open {
            let mut rows = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(xs);
            if let Some(star) = self.star_button(app) {
                rows = rows.with_child(star);
            }
            rows = rows.with_child(self.delete_button(app));
            let tray = Container::new(rows.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_border(app.theme.color(ColorToken::Border).into())
                .with_corner_radius(TRAY_RADIUS)
                .with_padding(EdgeInsets::uniform(TRAY_PADDING))
                .finish();
            self.menu = Some(Rc::new(RefCell::new(tray)));
        }

        // 3-dot menu, visible on hover or while the delete menu is open.
        if hover || menu_open {
            let ui_dots = Rc::clone(&self.ui);
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
                    ui.menu_open = !ui.menu_open;
                })
                .finish(),
            );
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

    /// The tray's box: hung from the 3-dot control it belongs to — its trailing
    /// edge on the control's, its own top `TRAY_GAP` under the card's bottom
    /// edge so it covers none of the card's own text — and clamped to the card's
    /// edges, so a tray wider than the card still starts inside the surface the
    /// card sits on. `None` until the tray has been laid out.
    fn menu_bounds(&self) -> Option<RectF> {
        let (origin, size) = (self.origin?, self.size?);
        let menu_size = self.menu_size?;
        self.menu.as_ref()?;
        let dots = self.dots_bounds()?;
        let left = origin.x();
        let right = (origin.x() + size.x - menu_size.x).max(left);
        let x = (dots.max_x() - menu_size.x).clamp(left, right);
        Some(rectf(
            x,
            origin.y() + size.y + TRAY_GAP,
            menu_size.x,
            menu_size.y,
        ))
    }

    /// Whether `position` is on the tray's surface — the tray's own box and the
    /// `TRAY_GAP`-tall strip between it and the card. The tray hangs a few
    /// points under the card, so a pointer walking from the control down to the
    /// tray crosses that strip: without it the card would read the crossing as
    /// the pointer leaving and put the tray away before it arrived.
    fn pointer_on_menu(&self, position: Vector2F) -> bool {
        let (Some(bounds), Some(origin), Some(size)) =
            (self.menu_bounds(), self.origin, self.size)
        else {
            return false;
        };
        contains(
            rectf(
                bounds.min_x(),
                origin.y() + size.y,
                bounds.width(),
                bounds.height() + TRAY_GAP,
            ),
            position,
        )
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
    fn star_button(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let on_toggle = self.on_toggle_star.clone()?;
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let starred = self.starred;
        let row = Flex::row()
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
            Button::new(row)
                .with_variant(ButtonVariant::Ghost)
                .with_corner_radius(0.0)
                .with_on_click(move || {
                    ui.borrow_mut().menu_open = false;
                    (on_toggle.borrow_mut())();
                })
                .finish(),
        )
    }

    fn delete_button(&self, app: &AppContext) -> Box<dyn Element> {
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let on_delete = self.on_delete.clone();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Icon::new("trash")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Error, app)
                    .finish(),
            )
            .with_child(
                Text::new("Delete agent")
                    .with_theme_color(ColorToken::Error, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        Button::new(row)
            .with_variant(ButtonVariant::Ghost)
            .with_corner_radius(0.0)
            .with_on_click(move || {
                if let Some(cb) = on_delete.as_ref() {
                    (cb.borrow_mut())();
                }
            })
            .finish()
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
                ui.menu_open = false;
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
    /// actions in a card of its own: hung under the card rather than in rows
    /// inside it, and inside the card's own width so the tray stays on the
    /// surface the card sits on.
    #[test]
    fn hover_renders_dots_and_menu_renders_delete() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            hover: true,
            menu_open: true,
        }));
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "A long last message",
            "5 min ago",
            ui,
            false,
        );
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let has_dots = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "dots-horizontal")
        });
        let has_delete = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text.contains("Delete agent"))
        });
        assert!(has_dots, "hover/menu card should render the 3-dot icon");
        assert!(has_delete, "open menu should render a 'Delete agent' action");

        let card = element.size().expect("the card is laid out");
        let delete_row = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Delete agent" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the delete row is drawn");
        let tray = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    corner_radius,
                    ..
                } if *corner_radius > 0.0
                    && rect.min_x() <= delete_row.x
                    && delete_row.x <= rect.max_x()
                    && rect.min_y() <= delete_row.y
                    && delete_row.y <= rect.max_y() =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the tray is a card of its own around its rows");
        assert!(
            tray.min_y() >= card.y,
            "the tray hangs under the card, not in rows inside it: {tray:?} vs card {card:?}"
        );
        assert!(
            tray.max_x() <= card.x,
            "and the tray stays inside the card's width: {tray:?} vs card {card:?}"
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
}
