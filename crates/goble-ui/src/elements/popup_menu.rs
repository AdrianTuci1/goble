use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill, Flex, Icon,
    LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, PointF, RectF, Size2F, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

const POPUP_MAX_WIDTH: f32 = 220.0;
const POPUP_ITEM_HEIGHT: f32 = 32.0;
const POPUP_GAP: f32 = 6.0;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupMenuPosition {
    Below,
    Above,
}

impl Default for PopupMenuPosition {
    fn default() -> Self {
        Self::Below
    }
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
    state: InteractiveState,
    panel: Option<Box<dyn Element>>,
    panel_size: Option<Vector2F>,
    panel_origin: Vector2F,
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
            state: InteractiveState::default(),
            panel: None,
            panel_size: None,
            panel_origin: Vector2F::zero(),
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

    fn build_panel(&mut self, app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let on_select = self.on_select.clone();
        let open = self.open.clone();
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
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

    fn rebuild(&mut self, app: &AppContext) {
        if !*self.open.borrow() {
            self.panel = None;
            self.panel_size = None;
            return;
        }
        if self.panel.is_none() {
            self.panel = Some(self.build_panel(app));
        }
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
            let panel_size = panel.layout(
                SizeConstraint::loose(vec2f(POPUP_MAX_WIDTH, 400.0)),
                ctx,
                app,
            );
            self.panel_size = Some(panel_size);
            let x = (trigger_size.x - panel_size.x).max(0.0);
            let y = match self.position {
                PopupMenuPosition::Below => trigger_size.y + POPUP_GAP,
                PopupMenuPosition::Above => -(panel_size.y + POPUP_GAP),
            };
            self.panel_origin = vec2f(x, y);
        }
        trigger_size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.trigger.paint(origin, ctx, app);
        if let Some(panel) = self.panel.as_mut() {
            panel.paint(origin + self.panel_origin, ctx, app);
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
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        if *self.open.borrow() {
            if let Some(panel_bounds) = self.panel_bounds() {
                if self.panel_is_inside(event, panel_bounds) {
                    return self
                        .panel
                        .as_mut()
                        .map(|p| p.dispatch_event(event, ctx, app))
                        .unwrap_or(false);
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
    use crate::elements::Empty;
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
}
