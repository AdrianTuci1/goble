//! The agent pane's bottom terminal dock.
//!
//! T3 Code's terminal drawer, reduced to what this app needs: a real PTY under
//! the chat/composer, opened from the pane header's terminal icon, with a
//! cursor-only resize edge and a 180pt minimum / 75%-of-pane maximum. It is a
//! dock, not a second surface: closing it leaves the agent chat exactly where
//! it was, and no pane kind or harness mode changes.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, CrossAxisAlignment, EdgeInsets, Element, EventContext, Expanded, Flex, Icon,
    LayoutContext, MainAxisSize, Padding, PaintContext, Point, SizeConstraint, Spacer, Text,
    TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::platform::window::CursorKind;
use goble_ui::theme::ColorToken;

use super::super::{UiActions, UiSnapshot};
use super::pane_session_snapshot;

/// The drawer's minimum height, T3 Code's `MIN_DRAWER_HEIGHT`.
pub(crate) const TERMINAL_DOCK_MIN_HEIGHT: f32 = 180.0;
/// The drawer may take at most this share of the pane's own height.
pub(crate) const TERMINAL_DOCK_MAX_SHARE: f32 = 0.75;
/// The resize edge at the dock's top: only the cursor changes here.
const RESIZE_EDGE: f32 = 6.0;
/// The drawer's own title row under the resize edge.
const DOCK_HEADER_HEIGHT: f32 = 28.0;

/// Build the open dock for `pane_id`: title row, resize edge and the live PTY.
pub(crate) fn build_terminal_dock(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    active: bool,
) -> Box<dyn Element> {
    let session = pane_session_snapshot(state, pane_id);
    let cwd = session.composer_path.clone();
    let body = crate::ui::terminal::build_terminal_dock(app, state, actions, pane_id, cwd, active);

    let on_toggle = actions.on_toggle_terminal_dock.clone();
    let title = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new("terminal")
                .with_size(13.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new("Terminal")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            TopbarButton::new(
                Icon::new("x")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(22.0)
            .with_on_click(move || (on_toggle.borrow_mut())(pane_id))
            .finish(),
        )
        .finish();
    let title = Padding::new(title, EdgeInsets::new(12.0, 0.0, 8.0, 0.0)).finish();
    let title = goble_ui::elements::ConstrainedBox::new(title)
        .with_min_height(DOCK_HEADER_HEIGHT)
        .finish();
    let content = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(title)
        .with_child(Expanded::new(body).finish())
        .finish();

    let controls = state.pane_controls.get(&pane_id).cloned();
    let setting = controls
        .as_ref()
        .map(|c| c.terminal_dock_height)
        .unwrap_or(240.0);
    let dragging = controls
        .as_ref()
        .map(|c| c.terminal_dock_dragging.clone())
        .unwrap_or_else(|| Rc::new(RefCell::new(false)));
    let on_height = actions.on_terminal_dock_height.clone();
    let resize: Rc<RefCell<dyn FnMut(f32)>> = Rc::new(RefCell::new(move |height| {
        (on_height.borrow_mut())(pane_id, height)
    }));
    Box::new(DockShell {
        child: Some(content),
        setting,
        dragging,
        on_height: resize,
        border: app.theme.color(ColorToken::Border),
        surface: app.theme.color(ColorToken::Bg),
        size: None,
        origin: None,
    })
}

/// The drawer's fixed-height shell: lays the content out at the chosen height,
/// paints its top rule and owns the pointer-only resize edge.
struct DockShell {
    child: Option<Box<dyn Element>>,
    setting: f32,
    dragging: Rc<RefCell<bool>>,
    on_height: Rc<RefCell<dyn FnMut(f32)>>,
    border: goble_ui::ColorU,
    surface: goble_ui::ColorU,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl DockShell {
    /// The drawer's height for this pass: the chosen height, never below the
    /// 180pt floor and never above 75% of the pane it is mounted in.
    fn height(&self, max: f32) -> f32 {
        let max = if max.is_finite() { max } else { f32::INFINITY };
        let floor = TERMINAL_DOCK_MIN_HEIGHT.min(max);
        let cap = (max * TERMINAL_DOCK_MAX_SHARE).max(floor);
        self.setting.max(floor).min(cap)
    }
}

impl Element for DockShell {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let height = self.height(constraint.max.y);
        let width = if constraint.max.x.is_finite() {
            constraint.max.x.max(constraint.min.x)
        } else {
            constraint.min.x
        };
        let child_size = match self.child.as_mut() {
            Some(child) => child.layout(
                SizeConstraint::new(vec2f(width, height), vec2f(width, height)),
                ctx,
                app,
            ),
            None => Vector2F::zero(),
        };
        let size = vec2f(width.max(child_size.x), height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(rectf(origin.x, origin.y, size.x, size.y), self.surface);
            renderer.fill_rect(rectf(origin.x, origin.y, size.x, 1.0), self.border);
        }
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
        }
        // Cursor-only resize: no grip is painted; the top edge changes the
        // pointer when hovered or dragged.
        let inside = ctx.cursor_inside
            && ctx.cursor_position.y >= origin.y
            && ctx.cursor_position.y <= origin.y + size.y
            && ctx.cursor_position.x >= origin.x
            && ctx.cursor_position.x <= origin.x + size.x;
        let hovered = inside && ctx.cursor_position.y <= origin.y + RESIZE_EDGE;
        if hovered || *self.dragging.borrow() {
            app.window_control.set_cursor(CursorKind::NsResize);
        } else if inside {
            // Inside the drawer but off the resize edge: the ordinary pointer.
            // Outside it, leave the cursor the element above painted.
            app.window_control.set_cursor(CursorKind::Default);
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
        let Some(origin) = self.origin.map(|p| vec2f(p.x(), p.y())) else {
            return false;
        };
        let size = self.size.unwrap_or(Vector2F::zero());
        let position = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => *position,
            _ => {
                return self
                    .child
                    .as_mut()
                    .is_some_and(|child| child.dispatch_event(event, ctx, app));
            }
        };
        let in_edge = position.x >= origin.x
            && position.x <= origin.x + size.x
            && position.y >= origin.y
            && position.y <= origin.y + RESIZE_EDGE;
        let dragging = *self.dragging.borrow();
        match event {
            DispatchedEvent::MouseDown { button, .. } if in_edge && *button == 0 => {
                *self.dragging.borrow_mut() = true;
                app.window_control.set_cursor(CursorKind::NsResize);
                true
            }
            DispatchedEvent::MouseMove { .. } if dragging => {
                let height = (origin.y + size.y - position.y).max(TERMINAL_DOCK_MIN_HEIGHT);
                (self.on_height.borrow_mut())(height);
                true
            }
            DispatchedEvent::MouseUp { .. } if dragging => {
                *self.dragging.borrow_mut() = false;
                true
            }
            _ => {
                if in_edge {
                    app.window_control.set_cursor(CursorKind::NsResize);
                }
                self.child
                    .as_mut()
                    .is_some_and(|child| child.dispatch_event(event, ctx, app))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(setting: f32) -> DockShell {
        DockShell {
            child: None,
            setting,
            dragging: Rc::new(RefCell::new(false)),
            on_height: Rc::new(RefCell::new(|_| {})),
            border: goble_ui::ColorU::new(0, 0, 0, 0),
            surface: goble_ui::ColorU::new(0, 0, 0, 0),
            size: None,
            origin: None,
        }
    }

    #[test]
    fn the_drawer_clamps_to_t3s_min_and_max() {
        assert_eq!(
            shell(80.0).height(1000.0),
            TERMINAL_DOCK_MIN_HEIGHT,
            "a short setting is raised to MIN_DRAWER_HEIGHT"
        );
        assert_eq!(
            shell(2000.0).height(1000.0),
            1000.0 * TERMINAL_DOCK_MAX_SHARE,
            "a tall setting is capped at 75% of the pane"
        );
        assert_eq!(
            shell(240.0).height(1000.0),
            240.0,
            "an ordinary height is kept"
        );
        assert_eq!(
            shell(300.0).height(200.0),
            180.0,
            "a pane shorter than the floor still gives the drawer its floor"
        );
    }
}
