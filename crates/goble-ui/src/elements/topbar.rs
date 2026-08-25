use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext,
    Fill, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point,
    SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

// A bit shorter on macOS so the actions sit vertically next to the traffic
// lights (the toolbar doubles as the OS titlebar there).
#[cfg(target_os = "macos")]
const TOPBAR_HEIGHT: f32 = 36.0;
#[cfg(not(target_os = "macos"))]
const TOPBAR_HEIGHT: f32 = 40.0;
const BUTTON_SIZE: f32 = 32.0;

// On macOS the real OS titlebar overlays the top of the window, with the
// traffic lights at the top-left. Leave room so the toolbar actions sit
// beside (not under) the traffic lights.
#[cfg(target_os = "macos")]
const TOPBAR_TRAFFIC_INSET: f32 = 76.0;
#[cfg(not(target_os = "macos"))]
const TOPBAR_TRAFFIC_INSET: f32 = 0.0;

    /// A premium application topbar matching the Tauri/Warp layout.
    ///
    /// Left side: menu toggle, threads button. Right side: inbox/agents
    /// button, settings button. The window's native titlebar (with working
    /// traffic lights) is provided by the OS, so this bar only hosts the
    /// toolbar actions and spans the full window width.
    pub struct Topbar {
        root: Box<dyn Element>,
        size: Option<Vector2F>,
        origin: Option<Point>,
    }

    impl Topbar {
        #[allow(clippy::too_many_arguments)]
        pub fn new(
            threads_active: bool,
            inbox_active: bool,
            settings_active: bool,
            on_menu: impl FnMut() + 'static,
            on_threads: impl FnMut() + 'static,
            on_inbox: impl FnMut() + 'static,
            on_settings: impl FnMut() + 'static,
            app: &AppContext,
        ) -> Self {
            // `spacing` is only used for the vertical toolbar padding, which is
            // dropped on macOS (the toolbar doubles as the OS titlebar).
            #[cfg_attr(target_os = "macos", allow(unused_variables))]
            let spacing = app.theme.spacing_px(SpacingToken::Md);
            let sm = app.theme.spacing_px(SpacingToken::Sm);

            let menu_icon = if threads_active || inbox_active || settings_active {
                "arrow-left"
            } else {
                "menu-01"
            };
            let menu_button = TopbarButton::new(
                Icon::new(menu_icon)
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_on_click(on_menu)
            .finish();

            let threads_button = TopbarButton::new(
                Icon::new("message-chat-square")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_active(threads_active)
            .with_on_click(on_threads)
            .finish();

            let left = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(menu_button)
                .with_child(threads_button)
                .finish();

            let inbox_button = TopbarButton::new(
                Icon::new("inbox-01")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_active(inbox_active)
            .with_on_click(on_inbox)
            .finish();

            let settings_button = TopbarButton::new(
                Icon::new("settings")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_active(settings_active)
            .with_on_click(on_settings)
            .finish();

            let right = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(inbox_button)
                .with_child(settings_button)
                .finish();

            let row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(left)
                .with_child(right)
                .finish();

            // On macOS the toolbar doubles as the titlebar: drop the vertical
            // padding so the actions sit vertically centered next to the
            // traffic lights. Elsewhere keep the padded toolbar look.
            #[cfg(target_os = "macos")]
            let vertical_padding = 0.0;
            #[cfg(not(target_os = "macos"))]
            let vertical_padding = spacing;

            let root = Container::new(ConstrainedBox::new(row).with_height(TOPBAR_HEIGHT).finish())
                .with_padding(EdgeInsets::new(
                    TOPBAR_TRAFFIC_INSET,
                    vertical_padding,
                    0.0,
                    vertical_padding,
                ))
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .finish();

            Self {
                root,
                size: None,
                origin: None,
            }
        }
    }

impl Element for Topbar {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.paint(origin, ctx, app);
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
        self.root.dispatch_event(event, ctx, app)
    }
}

/// A single icon button in the topbar with rounded hover/active backgrounds.
pub struct TopbarButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    active: bool,
    button_size: f32,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TopbarButton {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            active: false,
            button_size: BUTTON_SIZE,
            on_click: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.button_size = size;
        self
    }

    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn paint_background(&self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let rect = rectf(origin.x, origin.y, self.button_size, self.button_size);
        let bg = if self.active {
            app.theme.color(ColorToken::Selected)
        } else if ctx.hovered(rect) {
            app.theme.color(ColorToken::Hover)
        } else {
            return;
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(rect, bg, app.theme.radius_px() / 2.0);
        }
    }
}

impl Element for TopbarButton {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let _ = self.child.layout(
            SizeConstraint::tight(vec2f(self.button_size, self.button_size)),
            ctx,
            app,
        );
        let size = vec2f(self.button_size, self.button_size);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.paint_background(origin, ctx, app);
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let offset = vec2f(
            (self.button_size - child_size.x).max(0.0) / 2.0,
            (self.button_size - child_size.y).max(0.0) / 2.0,
        );
        self.child.paint(origin + offset, ctx, app);
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
        let cb = self.on_click.clone();
        let mut on_click = move || {
            if let Some(cb) = cb.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_click)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;

    #[test]
    fn topbar_button_paints_hover_when_cursor_over() {
        let app = AppContext::default();
        let mut button = TopbarButton::new(
            Icon::new("settings")
                .with_theme_color(ColorToken::Muted, &app)
                .finish(),
        );
        button.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );

        // Cursor over the button (top-left of the window).
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = vec2f(10.0, 10.0);
        paint_ctx.cursor_inside = true;
        button.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

        let hover_color = app.theme.color(ColorToken::Hover);
        assert!(
            commands.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { color, .. } if *color == hover_color
            )),
            "button should paint a hover background when the cursor is over it"
        );
    }

    #[test]
    fn topbar_layouts_non_zero() {
        let app = AppContext::default();
        let mut topbar = Topbar::new(false, false, false, || {}, || {}, || {}, || {}, &app);
        let size = topbar.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn topbar_button_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let app = AppContext::default();
        let mut button = TopbarButton::new(
            Icon::new("settings")
                .with_theme_color(ColorToken::Muted, &app)
                .finish(),
        )
        .with_on_click(move || *clicked_clone.borrow_mut() = true);

        button.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        button.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(button.dispatch_event(&down, &mut event_ctx, &app));
        assert!(button.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }
}
