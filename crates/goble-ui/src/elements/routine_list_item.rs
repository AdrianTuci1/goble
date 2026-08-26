use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Avatar, AvatarShape, Button, ButtonVariant, Container, CrossAxisAlignment,
    Element, Empty, EventContext, Expanded, Fill, Flex, Icon, LayoutContext, MainAxisAlignment,
    MainAxisSize, PaintContext, Point, SizeConstraint, Switch, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};
use crate::elements::routine_sidebar::{RoutineStatus, RoutineTrigger};

/// Per-card interaction state that must survive the per-frame element rebuild.
/// Owned by the app (a map keyed by routine id) and shared with the card
/// through `Rc<RefCell<_>>`, so hover / the delete menu persist across frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct RoutineCardUi {
    pub hover: bool,
    pub menu_open: bool,
}

pub struct RoutineListItem {
    id: String,
    name: String,
    trigger: RoutineTrigger,
    enabled: bool,
    status: RoutineStatus,
    selected: bool,
    ui: Rc<RefCell<RoutineCardUi>>,
    on_select: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_toggle_enabled: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    bg: crate::color::ColorU,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RoutineListItem {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        trigger: RoutineTrigger,
        enabled: bool,
        status: RoutineStatus,
        ui: Rc<RefCell<RoutineCardUi>>,
        selected: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            trigger,
            enabled,
            status,
            selected,
            ui,
            on_select: None,
            on_delete: None,
            on_toggle_enabled: None,
            root: None,
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

    pub fn with_on_toggle_enabled<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_enabled = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() {
            return;
        }
        let spacing = 10.0_f32;
        let xs = 6.0_f32;

        let ui = self.ui.borrow();
        let hover = ui.hover;
        let menu_open = ui.menu_open;

        let bg = if self.selected {
            app.theme.color(ColorToken::Selected)
        } else if hover {
            app.theme.color(ColorToken::Hover)
        } else {
            app.theme.color(ColorToken::Surface)
        };
        self.bg = bg;

        let status_color = match self.status {
            RoutineStatus::Running => ColorToken::Accent,
            RoutineStatus::Success => ColorToken::Success,
            RoutineStatus::Error => ColorToken::Error,
            RoutineStatus::Stopped => ColorToken::Muted,
            RoutineStatus::Idle => ColorToken::Muted,
        };

        let status_dot = Container::new(Empty::new().finish())
            .with_background(Fill::Solid(app.theme.color(status_color)))
            .with_corner_radius(4.0)
            .finish();

        let meta_text = format!("{} • {}",
            self.trigger.label(),
            if self.enabled { "enabled" } else { "disabled" }
        );

        let name_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_spacing(xs)
            .with_child(
                Text::new(self.name.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(status_dot)
            .finish();

        let meta_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Text::new(meta_text)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish();

        let text_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(name_row)
            .with_child(meta_row)
            .finish();

        let avatar = Avatar::new(self.name.clone())
            .with_size(28.0)
            .with_shape(AvatarShape::Squircle)
            .with_theme_background(ColorToken::Muted, app)
            .with_theme_foreground(ColorToken::Text, app)
            .finish();

        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(avatar)
            .with_child(Expanded::new(text_column).finish());

        // 3-dot menu, visible on hover or while the delete menu is open.
        if hover || menu_open {
            let ui_dots = Rc::clone(&self.ui);
            let dots = TopbarButton::new(
                Icon::new("dots-horizontal")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(26.0)
            .with_on_click(move || {
                let mut ui = ui_dots.borrow_mut();
                ui.menu_open = !ui.menu_open;
            })
            .finish();
            row = row.with_child(dots);
        }

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_child(row.finish());

        // Action menu, shown as a row under the card while open.
        if menu_open {
            let toggle = self.toggle_enabled_button(app);
            let delete = self.delete_button(app);
            let del_row = Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(toggle)
                .with_child(delete)
                .finish();
            column = column.with_child(
                Container::new(del_row)
                    .with_padding(crate::style::EdgeInsets::new(0.0, 0.0, xs, 0.0))
                    .finish(),
            );
        }

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(bg))
                .with_corner_radius(app.theme.radius_px())
                .finish(),
        );
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
                Text::new("Delete routine")
                    .with_theme_color(ColorToken::Error, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        Button::new(row)
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                if let Some(cb) = on_delete.as_ref() {
                    (cb.borrow_mut())();
                }
            })
            .finish()
    }

    fn toggle_enabled_button(&self, app: &AppContext) -> Box<dyn Element> {
        let on_toggle = self.on_toggle_enabled.clone();
        let label = if self.enabled { "Disable" } else { "Enable" };
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Text::new(label)
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(Switch::new().with_checked(self.enabled).with_size(vec2f(14.0, 14.0)).finish())
            .finish();
        Button::new(row)
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                if let Some(cb) = on_toggle.as_ref() {
                    (cb.borrow_mut())();
                }
            })
            .finish()
    }
}

impl Element for RoutineListItem {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));

        if let Some(b) = self.size.map(|s| rectf(origin.x, origin.y, s.x, s.y)) {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.clip_rect(b);
            }
        }
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }

        let ui = self.ui.borrow();
        if !ui.hover && !ui.menu_open {
            if let Some(size) = self.size {
                let fade_w = 24.0_f32;
                if size.x > fade_w {
                    let rect = rectf(origin.x + size.x - fade_w, origin.y, fade_w, size.y);
                    if let Some(renderer) = ctx.renderer.as_mut() {
                        renderer.fill_rect_fade_right(rect, self.bg, 0.0);
                    }
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

        if let Some(root) = self.root.as_mut() {
            if root.dispatch_event(event, ctx, app) {
                return true;
            }
        }

        if let DispatchedEvent::MouseMove { position } = event {
            let inside = contains(bounds, *position);
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

pub fn card_bounds(item: &RoutineListItem) -> Option<RectF> {
    item.bounds()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn routine_list_item_layouts() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(RoutineCardUi::default()));
        let mut item = RoutineListItem::new(
            "r1",
            "Morning summary",
            RoutineTrigger::Manual,
            true,
            RoutineStatus::Idle,
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
    fn routine_list_item_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(RoutineCardUi::default()));
        let mut item = RoutineListItem::new(
            "r1",
            "Morning summary",
            RoutineTrigger::Manual,
            true,
            RoutineStatus::Idle,
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
        let ui = Rc::new(RefCell::new(RoutineCardUi::default()));
        let mut item = RoutineListItem::new(
            "r1",
            "Morning summary",
            RoutineTrigger::Manual,
            true,
            RoutineStatus::Idle,
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
}
