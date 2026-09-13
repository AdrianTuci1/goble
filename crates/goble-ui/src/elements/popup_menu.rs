use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill, Flex, Icon,
    LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, PointF, RectF, Size2F, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

const POPUP_MAX_WIDTH: f32 = 220.0;
const POPUP_ITEM_HEIGHT: f32 = 32.0;
const POPUP_ITEM_SPACING: f32 = 2.0;
const POPUP_GAP: f32 = 6.0;
/// The gap between the hovered row and its tray (warp-new's action sidecar).
const TRAY_GAP: f32 = 4.0;

/// One entry in a [`PopupMenu`].
#[derive(Clone, Debug)]
pub struct PopupMenuItem {
    pub label: String,
    pub icon: Option<&'static str>,
    pub selected: bool,
    pub disabled: bool,
}

impl PopupMenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            selected: false,
            disabled: false,
        }
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn selected(mut self) -> Self {
        self.selected = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// Where a [`PopupMenu`] panel appears relative to its trigger.
///
/// The `*End` variants align the panel's trailing edge with the trigger's, so a
/// trigger that sits near the right edge of its pane opens the panel leftwards,
/// inside the window, instead of spilling past the edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupMenuPosition {
    Below,
    Above,
    BelowEnd,
    AboveEnd,
}

impl Default for PopupMenuPosition {
    fn default() -> Self {
        Self::Below
    }
}

/// A tray drawn beside the row under the pointer, following warp-new's action
/// sidecar: it appears next to the hovered item (to its right, or to its left
/// when it would leave the window) and carries that item's own actions, so a
/// menu row can offer more than the one thing a click on it does.
struct HoverTray {
    width: f32,
    /// Content for the hovered item's index, or `None` for an item that carries
    /// no tray.
    build: Rc<dyn Fn(usize, &AppContext) -> Option<Box<dyn Element>>>,
}

/// A trigger that opens a floating item menu.
///
/// The open flag is app-owned (`Rc<RefCell<bool>>`) so it survives the
/// per-frame element rebuild. While open the panel is drawn over the trigger's
/// neighbours; clicking outside it (or selecting an item) closes it.
pub struct PopupMenu {
    trigger: Box<dyn Element>,
    items: Vec<PopupMenuItem>,
    open: Rc<RefCell<bool>>,
    position: PopupMenuPosition,
    on_select: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    hover_tray: Option<HoverTray>,
    /// The item the pointer is over, shared with the app. The tree is rebuilt
    /// every frame, so this cannot live in the element: it is written by
    /// `paint` (where hover is decided) and read by the next frame's `layout`,
    /// which is what builds the tray.
    hover_index: Rc<RefCell<Option<usize>>>,
    state: InteractiveState,
    panel: Option<Box<dyn Element>>,
    panel_size: Option<Vector2F>,
    panel_origin: Vector2F,
    tray: Option<Box<dyn Element>>,
    tray_size: Option<Vector2F>,
    tray_origin: Vector2F,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PopupMenu {
    pub fn new(trigger: Box<dyn Element>, items: Vec<PopupMenuItem>) -> Self {
        Self {
            trigger,
            items,
            open: Rc::new(RefCell::new(false)),
            position: PopupMenuPosition::default(),
            on_select: None,
            hover_tray: None,
            hover_index: Rc::new(RefCell::new(None)),
            state: InteractiveState::default(),
            panel: None,
            panel_size: None,
            panel_origin: Vector2F::zero(),
            tray: None,
            tray_size: None,
            tray_origin: Vector2F::zero(),
            size: None,
            origin: None,
        }
    }

    pub fn with_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.open = open;
        self
    }

    pub fn with_position(mut self, position: PopupMenuPosition) -> Self {
        self.position = position;
        self
    }

    pub fn with_on_select<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Show a tray beside the item under the pointer, built from that item's
    /// index (`None` for items that carry no tray). The tray is laid out at
    /// `width` and hangs from the hovered row's top edge.
    ///
    /// Hover is decided at paint time (see [`PaintContext::hovered`]) because
    /// the tree is rebuilt every frame, so the tray for a row appears on the
    /// frame after the pointer reaches it. Pass the app's own cell to
    /// [`Self::with_hover_index`] when the menu is rebuilt every frame, as the
    /// app's menus are, otherwise that frame-to-frame memory is lost with the
    /// element.
    pub fn with_hover_tray<F>(mut self, width: f32, build: F) -> Self
    where
        F: Fn(usize, &AppContext) -> Option<Box<dyn Element>> + 'static,
    {
        self.hover_tray = Some(HoverTray { width, build: Rc::new(build) });
        self
    }

    /// Share the item-under-the-pointer cell with the app, so the tray survives
    /// the per-frame rebuild (the open flag is app-owned for the same reason).
    pub fn with_hover_index(mut self, hover_index: Rc<RefCell<Option<usize>>>) -> Self {
        self.hover_index = hover_index;
        self
    }

    /// The panel's inner padding (the gap between the panel edge and a row).
    fn panel_padding(app: &AppContext) -> f32 {
        app.theme.spacing_px(SpacingToken::Sm)
    }

    pub fn is_open(&self) -> bool {
        *self.open.borrow()
    }

    fn panel_bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.panel_size?;
        Some(RectF::new(
            PointF::new(origin.x() + self.panel_origin.x, origin.y() + self.panel_origin.y),
            Size2F::new(size.x, size.y),
        ))
    }

    fn tray_bounds(&self) -> Option<RectF> {
        let size = self.tray_size?;
        let origin = self.origin?;
        Some(RectF::new(
            PointF::new(origin.x() + self.tray_origin.x, origin.y() + self.tray_origin.y),
            Size2F::new(size.x, size.y),
        ))
    }

    /// The bounds of item `index`'s row, in window coordinates. Rows are a
    /// fixed height with a fixed spacing between them, so a row's rect follows
    /// from the panel's geometry without asking the row element.
    fn row_bounds(&self, index: usize, app: &AppContext) -> Option<RectF> {
        let origin = self.origin?;
        let panel_size = self.panel_size?;
        let pad = Self::panel_padding(app);
        let x = origin.x() + self.panel_origin.x + pad;
        let y =
            origin.y() + self.panel_origin.y + pad + index as f32 * (POPUP_ITEM_HEIGHT + POPUP_ITEM_SPACING);
        Some(RectF::new(
            PointF::new(x, y),
            Size2F::new((panel_size.x - pad * 2.0).max(0.0), POPUP_ITEM_HEIGHT),
        ))
    }

    /// Build the hovered row's tray and place it beside that row: to its right,
    /// or to its left when it would otherwise leave the window (warp-new's
    /// overflow rule). Called from `layout`, so the tray is in the tree and can
    /// take a click on the frame after the pointer reached the row.
    fn build_tray(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) {
        self.tray = None;
        self.tray_size = None;
        self.tray_origin = Vector2F::zero();

        let (Some(index), Some(hover_tray)) = (*self.hover_index.borrow(), self.hover_tray.as_ref())
        else {
            return;
        };
        let Some(content) = (hover_tray.build)(index, app) else {
            return;
        };
        let Some(panel_size) = self.panel_size else {
            return;
        };
        let pad = Self::panel_padding(app);
        // Menu-relative, like `panel_origin`: the tray hangs from the hovered
        // row's top edge and sits just past the panel's trailing edge.
        let row_top = self.panel_origin.y + pad + index as f32 * (POPUP_ITEM_HEIGHT + POPUP_ITEM_SPACING);
        let panel_right = self.panel_origin.x + panel_size.x;

        let mut tray = content;
        let max_height = if constraint.max.y.is_finite() {
            constraint.max.y
        } else {
            POPUP_MAX_WIDTH
        };
        let size = tray.layout(
            SizeConstraint::loose(vec2f(hover_tray.width, max_height)),
            ctx,
            app,
        );

        // The overflow test needs the menu's position on screen, which layout
        // does not have: it comes from this element's own last paint, so a menu
        // the app rebuilds every frame knows only the panel's geometry. An
        // unbounded available width (a shrink-to-content row, as the topbar's
        // is) means there is no edge to run into, so the tray opens rightwards.
        let menu_left = self.origin.map(|o| o.x()).unwrap_or(0.0);
        let fits_right = !constraint.max.x.is_finite()
            || menu_left + panel_right + TRAY_GAP + size.x <= constraint.max.x;
        let x = if fits_right {
            panel_right + TRAY_GAP
        } else {
            self.panel_origin.x - TRAY_GAP - size.x
        };
        self.tray_origin = vec2f(x, row_top);
        self.tray_size = Some(size);
        self.tray = Some(tray);
    }

    /// Re-read which row the pointer is over from the paint-time cursor, which
    /// is where hover lives in this framework. The pointer over the tray itself
    /// keeps the tray open, so it can be moved onto and clicked.
    fn track_hover(&mut self, ctx: &PaintContext, app: &AppContext) {
        let over_row = (0..self.items.len())
            .find(|i| self.row_bounds(*i, app).is_some_and(|row| ctx.hovered(row)));
        if let Some(index) = over_row {
            *self.hover_index.borrow_mut() = Some(index);
            return;
        }
        let over_tray = self.tray_bounds().is_some_and(|tray| ctx.hovered(tray));
        if !over_tray {
            *self.hover_index.borrow_mut() = None;
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        if !*self.open.borrow() {
            self.panel = None;
            self.panel_size = None;
            self.tray = None;
            self.tray_size = None;
            *self.hover_index.borrow_mut() = None;
            return;
        }
        if self.panel.is_none() {
            self.panel = Some(self.build_panel(app));
        }
    }

    fn build_panel(&mut self, app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let on_select = self.on_select.clone();
        let open = self.open.clone();
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(POPUP_ITEM_SPACING);
        for (index, item) in self.items.iter().enumerate() {
            let label = item.label.clone();
            let icon = item.icon.clone();
            let selected = item.selected;
            let disabled = item.disabled;
            let is_open = open.clone();
            let cb = on_select.clone();
            let row = PopupMenuItemView::new(
                label,
                icon,
                selected,
                disabled,
                move || {
                    if !disabled {
                        if let Some(cb) = cb.as_ref() {
                            (cb.borrow_mut())(index);
                        }
                        *is_open.borrow_mut() = false;
                    }
                },
            )
            .finish();
            column = column.with_child(row);
        }
        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(sm))
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .with_corner_radius(6.0)
            .finish()
    }
}

impl Default for PopupMenu {
    fn default() -> Self {
        Self::new(
            Box::new(crate::elements::Empty::new()),
            Vec::new(),
        )
    }
}

impl Element for PopupMenu {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let trigger_size = self.trigger.layout(constraint, ctx, app);
        self.size = Some(trigger_size);

        self.panel_origin = Vector2F::zero();
        if let Some(panel) = self.panel.as_mut() {
            // Keep the panel within the horizontal space actually available so
            // it re-anchors under its trigger and is never clipped/cut off when
            // the window or composer is resized (narrower than the max width).
            let available_width = if constraint.max.x.is_finite() {
                constraint.max.x
            } else {
                POPUP_MAX_WIDTH
            };
            let panel_size = panel.layout(
                SizeConstraint::loose(vec2f(available_width.min(POPUP_MAX_WIDTH), 400.0)),
                ctx,
                app,
            );
            self.panel_size = Some(panel_size);
            // Default: right-align a panel narrower than its trigger so it sits
            // under the trigger, otherwise align to the trigger's leading edge.
            // A trailing-aligned panel keeps its right edge on the trigger's,
            // which is what lets a control near the window's right edge open to
            // the left rather than off-screen.
            let trailing = matches!(
                self.position,
                PopupMenuPosition::BelowEnd | PopupMenuPosition::AboveEnd
            );
            let mut x = if trailing {
                trigger_size.x - panel_size.x
            } else {
                (trigger_size.x - panel_size.x).max(0.0)
            };
            // Clamp so the whole panel stays within the available width (no
            // spill past the composer/window edge on a narrow or resized layout).
            let max_x = (constraint.max.x - panel_size.x).max(0.0);
            x = x.min(max_x);
            let y = match self.position {
                PopupMenuPosition::Below | PopupMenuPosition::BelowEnd => {
                    trigger_size.y + POPUP_GAP
                }
                PopupMenuPosition::Above | PopupMenuPosition::AboveEnd => {
                    -(panel_size.y + POPUP_GAP)
                }
            };
            self.panel_origin = vec2f(x, y);
        }
        self.build_tray(constraint, ctx, app);
        trigger_size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.trigger.paint(origin, ctx, app);
        if let Some(panel) = self.panel.as_mut() {
            panel.paint(origin + self.panel_origin, ctx, app);
        }
        if let Some(tray) = self.tray.as_mut() {
            tray.paint(origin + self.tray_origin, ctx, app);
        }
        self.track_hover(ctx, app);
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
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        if *self.open.borrow() {
            // The tray is drawn beside the panel, so it sits outside the panel's
            // own bounds: route events there first, and consume them so a click
            // on the tray's controls neither closes the menu nor falls through
            // to the "outside the panel" close below.
            if let Some(position) = event_position(event) {
                if self.tray_bounds().is_some_and(|tray| contains(tray, position)) {
                    let _ = self
                        .tray
                        .as_mut()
                        .map(|t| t.dispatch_event(event, ctx, app));
                    return true;
                }
            }
            if let Some(panel_bounds) = self.panel_bounds() {
                if self.panel_is_inside(event, panel_bounds) {
                    return self
                        .panel
                        .as_mut()
                        .map(|p| p.dispatch_event(event, ctx, app))
                        .unwrap_or(false);
                }
            }
            // A press that lands on the trigger itself should toggle the menu
            // closed, not be swallowed by the "outside the panel" close below
            // (which would otherwise let the release re-open it).
            if let Some(position) = event_position(event) {
                if contains(bounds, position) {
                    let cb = open_toggle_closure(self.open.clone());
                    let mut toggle = move || (cb.borrow_mut())();
                    return handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle);
                }
            }
            // Clicking outside the panel (but somewhere in the window) closes it.
            if matches!(event, DispatchedEvent::MouseDown { .. }) {
                *self.open.borrow_mut() = false;
            }
            return false;
        }

        let cb = open_toggle_closure(self.open.clone());
        let mut toggle = move || (cb.borrow_mut())();
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle)
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

fn open_toggle_closure(open: Rc<RefCell<bool>>) -> Rc<RefCell<dyn FnMut()>> {
    Rc::new(RefCell::new(move || {
        let current = *open.borrow();
        *open.borrow_mut() = !current;
    }))
}

trait PanelInside {
    fn panel_is_inside(&self, event: &DispatchedEvent, bounds: RectF) -> bool;
}

impl PanelInside for PopupMenu {
    fn panel_is_inside(&self, event: &DispatchedEvent, bounds: RectF) -> bool {
        match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => {
                bounds.contains(PointF::new(position.x, position.y))
            }
            _ => false,
        }
    }
}

/// A single clickable row inside the popup panel.
struct PopupMenuItemView {
    label: String,
    icon: Option<&'static str>,
    selected: bool,
    disabled: bool,
    state: InteractiveState,
    on_select: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    content: Option<Box<dyn Element>>,
    content_origin: Vector2F,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PopupMenuItemView {
    fn new(
        label: String,
        icon: Option<&'static str>,
        selected: bool,
        disabled: bool,
        on_select: impl FnMut() + 'static,
    ) -> Self {
        Self {
            label,
            icon,
            selected,
            disabled,
            state: InteractiveState::default(),
            on_select: Some(Rc::new(RefCell::new(on_select))),
            content: None,
            content_origin: Vector2F::zero(),
            size: None,
            origin: None,
        }
    }

    fn build_content(&mut self, app: &AppContext) {
        if self.content.is_some() {
            return;
        }
        let text_color = if self.disabled {
            ColorToken::Muted
        } else if self.selected {
            ColorToken::Accent
        } else {
            ColorToken::Text
        };
        let text = Text::new(self.label.clone())
            .with_theme_color(text_color, app)
            .with_font_size(12.0);
        let mut content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0);
        if let Some(icon_name) = self.icon.clone() {
            content = content.with_child(
                Icon::new(icon_name)
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        content = content.with_child(text.finish());
        self.content = Some(content.finish());
    }
}

impl Element for PopupMenuItemView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.build_content(app);
        let size = vec2f(constraint.max.x.min(POPUP_MAX_WIDTH), POPUP_ITEM_HEIGHT);
        self.size = Some(size);
        let content = self.content.as_mut().unwrap();
        let _ = content.layout(
            SizeConstraint::loose(vec2f(POPUP_MAX_WIDTH, POPUP_ITEM_HEIGHT)),
            ctx,
            app,
        );
        let content_size = content.size().unwrap_or(Vector2F::zero());
        self.content_origin = vec2f(8.0, (size.y - content_size.y).max(0.0) / 2.0);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let hit = rectf(origin.x, origin.y, size.x, size.y);
        let bg = if self.selected && !self.disabled {
            Some(app.theme.color(ColorToken::Selected))
        } else if ctx.hovered(hit) && !self.disabled {
            Some(app.theme.color(ColorToken::Hover))
        } else {
            None
        };
        if let (Some(color), Some(renderer)) = (bg, ctx.renderer.as_mut()) {
            renderer.fill_rounded_rect(hit, color, 4.0);
        }
        if let Some(content) = self.content.as_mut() {
            content.paint(origin + self.content_origin, ctx, app);
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
        _app: &AppContext,
    ) -> bool {
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };
        let cb = self.on_select.clone();
        let mut on_select = move || {
            if let Some(cb) = cb.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_select)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Button, Empty, Text};
    use crate::geometry::vec2f;

    fn trigger() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(40.0, 32.0)).finish()
    }

    fn click(menu: &mut PopupMenu, x: f32, y: f32, app: &AppContext) {
        let mut ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(x, y),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(x, y),
            button: 0,
        };
        menu.dispatch_event(&down, &mut ctx, app);
        menu.dispatch_event(&up, &mut ctx, app);
    }

    #[test]
    fn opens_on_trigger_click_and_selects_item() {
        let items = vec![
            PopupMenuItem::new("One").with_icon("cpu"),
            PopupMenuItem::new("Two"),
        ];
        let selected = Rc::new(RefCell::new(None));
        let selected_clone = selected.clone();
        let mut menu = PopupMenu::new(trigger(), items)
            .with_position(PopupMenuPosition::Above)
            .with_on_select(move |index| *selected_clone.borrow_mut() = Some(index));
        let app = AppContext::default();

        menu.layout(
            SizeConstraint::loose(vec2f(200.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        // Click on the trigger opens the menu.
        click(&mut menu, 10.0, 16.0, &app);
        assert!(menu.is_open());

        // The app rebuilds the tree each frame, so layout again to build the panel.
        menu.layout(
            SizeConstraint::loose(vec2f(200.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        // The panel appears above the trigger (negative y). Click the first
        // item, which sits a few pixels into the panel.
        click(&mut menu, 10.0, -64.0, &app);
        assert!(!menu.is_open());
        assert_eq!(*selected.borrow(), Some(0));
    }

    #[test]
    fn a_trailing_panel_aligns_its_right_edge_with_the_trigger() {
        let items = vec![
            PopupMenuItem::new("A long label"),
            PopupMenuItem::new("Another item"),
        ];
        let app = AppContext::default();
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::new(RefCell::new(true)))
            .with_position(PopupMenuPosition::BelowEnd);
        // The trigger is 40 wide and sits at the right end of a 360 wide row,
        // which is the dots button at the pane's right edge.
        menu.layout(
            SizeConstraint::loose(vec2f(360.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let size = menu.panel_size.expect("panel built");
        assert!(size.x > 40.0, "the panel is wider than its trigger");
        assert!(
            (menu.panel_origin.x + size.x - 40.0).abs() < 0.01,
            "the panel's right edge sits on the trigger's: origin {} size {}",
            menu.panel_origin.x,
            size.x
        );
    }

    #[test]
    fn panel_reanchors_and_stays_within_available_width_on_resize() {
        let items = vec![
            PopupMenuItem::new("A long label that wraps"),
            PopupMenuItem::new("Another long item"),
        ];
        let open = Rc::new(RefCell::new(true));
        let mut menu = PopupMenu::new(trigger(), items).with_open(open);
        let app = AppContext::default();

        // Narrow layout: the panel must not overflow the available width.
        menu.layout(
            SizeConstraint::loose(vec2f(120.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let narrow_size = menu.panel_size.expect("panel built");
        assert!(
            narrow_size.x <= 120.0,
            "panel width {} exceeds available width",
            narrow_size.x
        );
        assert!(menu.panel_origin.x >= 0.0);
        assert!(
            menu.panel_origin.x + narrow_size.x <= 120.0,
            "panel overflows available width on narrow layout"
        );

        // Widened layout: the panel re-anchors under the trigger and fits.
        menu.layout(
            SizeConstraint::loose(vec2f(360.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let wide_size = menu.panel_size.expect("panel built");
        assert!(wide_size.x <= POPUP_MAX_WIDTH);
        assert!(
            menu.panel_origin.x + wide_size.x <= 360.0,
            "panel overflows available width after resize"
        );
    }

    /// A menu whose second item carries a tray with a "Make default" button and
    /// whose first item carries none.
    fn menu_with_a_tray(clicked: Rc<RefCell<usize>>) -> (PopupMenu, Rc<RefCell<bool>>) {
        let open = Rc::new(RefCell::new(true));
        let items = vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")];
        let menu = PopupMenu::new(trigger(), items)
            .with_open(Rc::clone(&open))
            .with_hover_tray(180.0, move |index, _app| {
                (index == 1).then(|| {
                    let clicked = Rc::clone(&clicked);
                    Button::new(Text::new("Make default").finish())
                        .with_on_click(move || *clicked.borrow_mut() = index)
                        .finish()
                })
            });
        (menu, open)
    }

    /// Put the pointer over item `index` and lay the menu out again, so the tray
    /// that pointer calls for is in the tree (hover is read at paint time, one
    /// frame before the tray is built).
    fn hover_item(menu: &mut PopupMenu, index: usize, app: &AppContext, width: f32, origin: Vector2F) {
        let cursor = menu
            .row_bounds(index, app)
            .map(|row| vec2f(row.min_x() + 4.0, row.min_y() + 4.0))
            .expect("the panel is laid out, so its rows have bounds");
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = cursor;
        paint_ctx.cursor_inside = true;
        menu.paint(origin, &mut paint_ctx, app);
        menu.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        menu.paint(origin, &mut paint_ctx, app);
    }

    fn lay_out_open(menu: &mut PopupMenu, app: &AppContext, width: f32, origin: Vector2F) {
        menu.layout(
            SizeConstraint::loose(vec2f(width, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        menu.paint(origin, &mut PaintContext::default(), app);
    }

    #[test]
    fn hovering_a_row_shows_its_tray_beside_it() {
        let clicked = Rc::new(RefCell::new(usize::MAX));
        let (mut menu, _open) = menu_with_a_tray(Rc::clone(&clicked));
        let app = AppContext::default();
        let at = vec2f(0.0, 0.0);
        lay_out_open(&mut menu, &app, 600.0, at);
        assert!(
            menu.tray.is_none(),
            "no tray before the pointer is over a row"
        );

        hover_item(&mut menu, 1, &app, 600.0, at);
        let tray_size = menu
            .tray
            .as_ref()
            .and_then(|tray| tray.size())
            .expect("the hovered row's tray is laid out");
        let panel = menu.panel_bounds().expect("the panel has bounds");
        let row = menu.row_bounds(1, &app).expect("the hovered row");

        // Beside the panel the row belongs to, one `TRAY_GAP` away, hanging from
        // the row's top edge.
        let tray = menu.tray_bounds().expect("the tray has bounds");
        assert!(
            (tray.min_x() - (panel.max_x() + TRAY_GAP)).abs() < 0.01,
            "the tray sits {TRAY_GAP} px right of the panel (tray at {}, panel ends at {})",
            tray.min_x(),
            panel.max_x()
        );
        assert!(
            !tray.intersects(&panel),
            "and does not overlap the panel it belongs to"
        );
        assert!(
            (tray.min_y() - row.min_y()).abs() < 0.01,
            "the tray hangs from the hovered row's top edge (tray {}, row {})",
            tray.min_y(),
            row.min_y()
        );
        assert!(tray_size.x > 0.0 && tray_size.y > 0.0);

        // An item whose builder returns `None` shows no tray.
        hover_item(&mut menu, 0, &app, 600.0, at);
        assert!(
            menu.tray.is_none(),
            "the first item carries no tray, so none is drawn for it"
        );
    }

    #[test]
    fn a_tray_click_fires_its_own_button_and_keeps_the_menu_open() {
        let clicked = Rc::new(RefCell::new(usize::MAX));
        let (mut menu, open) = menu_with_a_tray(Rc::clone(&clicked));
        let app = AppContext::default();
        let at = vec2f(0.0, 0.0);
        lay_out_open(&mut menu, &app, 600.0, at);
        hover_item(&mut menu, 1, &app, 600.0, at);

        // Press on the tray's button, rebuild the tree the way a redraw does
        // (the pointer is still on the tray, which is what keeps it in the tree)
        // and release on it.
        let mut ctx = EventContext::default();
        let mut press_and_release = |menu: &mut PopupMenu| {
            let tray = menu.tray_bounds().expect("the tray has bounds");
            let pos = vec2f(tray.min_x() + 8.0, tray.min_y() + 8.0);
            let down = DispatchedEvent::MouseDown { position: pos, button: 0 };
            let up = DispatchedEvent::MouseUp { position: pos, button: 0 };
            let _ = menu.dispatch_event(&down, &mut ctx, &app);
            let _ = menu.dispatch_event(&up, &mut ctx, &app);
        };
        press_and_release(&mut menu);
        assert_eq!(*clicked.borrow(), 1, "the tray's own button took the click");
        assert!(*open.borrow(), "a click on the tray keeps the menu open");

        // The rows still select, and selecting one still closes the menu.
        let row = menu.row_bounds(0, &app).expect("row zero");
        click(&mut menu, row.min_x() + 4.0, row.min_y() + 4.0, &app);
        assert_eq!(
            *clicked.borrow(),
            1,
            "a click on the row is not the tray's button"
        );
        assert!(!*open.borrow(), "selecting a row closes the menu");
    }

    #[test]
    fn a_tray_that_would_leave_the_window_renders_to_the_left() {
        let app = AppContext::default();
        let width = 600.0;
        // A trailing menu at the window's right edge (the topbar's "▾" control):
        // its panel opens leftwards from the trigger, so a tray to the panel's
        // right would leave the window.
        let at = vec2f(width - 40.0, 0.0);
        let open = Rc::new(RefCell::new(true));
        let items = vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")];
        let mut menu = PopupMenu::new(trigger(), items)
            .with_open(open)
            .with_position(PopupMenuPosition::BelowEnd)
            .with_hover_tray(180.0, move |index, _app| {
                (index == 1).then(|| Button::new(Text::new("Make default").finish()).finish())
            });
        lay_out_open(&mut menu, &app, width, at);
        hover_item(&mut menu, 1, &app, width, at);

        let tray = menu.tray_bounds().expect("the tray has bounds");
        let panel = menu.panel_bounds().expect("the panel has bounds");
        assert!(
            tray.max_x() <= panel.min_x(),
            "the tray renders left of the panel when it does not fit on the right: \
             tray {}..{}, panel starts at {}",
            tray.min_x(),
            tray.max_x(),
            panel.min_x()
        );
        assert!(
            tray.min_x() >= 0.0 && tray.max_x() <= width,
            "and stays inside the window: {}..{}",
            tray.min_x(),
            tray.max_x()
        );
    }

    #[test]
    fn the_tray_survives_the_per_frame_rebuild_through_the_shared_hover_cell() {
        // The app builds a new element tree every frame, so the row under the
        // pointer cannot live in the menu element: it is an app-owned cell,
        // written at paint and read by the next frame's layout.
        let hover: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let app = AppContext::default();
        let build = |hover: &Rc<RefCell<Option<usize>>>| {
            PopupMenu::new(
                trigger(),
                vec![PopupMenuItem::new("One"), PopupMenuItem::new("Two")],
            )
            .with_open(Rc::new(RefCell::new(true)))
            .with_hover_index(Rc::clone(hover))
            .with_hover_tray(180.0, |index, _app| {
                (index == 1).then(|| Button::new(Text::new("Make default").finish()).finish())
            })
        };

        // This frame: the pointer is over the second row, which painting records.
        let mut frame = build(&hover);
        lay_out_open(&mut frame, &app, 600.0, vec2f(0.0, 0.0));
        let row = frame.row_bounds(1, &app).expect("row one");
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = vec2f(row.min_x() + 4.0, row.min_y() + 4.0);
        paint_ctx.cursor_inside = true;
        frame.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        assert_eq!(*hover.borrow(), Some(1), "the frame records the hovered row");
        drop(frame);

        // The next frame is a brand-new menu; the shared cell carries the hover.
        let mut next_frame = build(&hover);
        next_frame.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(
            next_frame.tray.is_some(),
            "the rebuilt menu still draws the tray for the row under the pointer"
        );
    }
}
