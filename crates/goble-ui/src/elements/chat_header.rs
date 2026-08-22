use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Icon, LayoutContext,
    MainAxisAlignment, PaintContext, Point, SizeConstraint, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

/// A header for an individual chat.
///
/// Shows the conversation title on the left and a toggle button for the right
/// chat sidebar on the right. The toggle button is only rendered when
/// `with_sidebar_toggle` is called.
pub struct ChatHeader {
    title: String,
    app: AppContext,
    sidebar_visible: bool,
    on_toggle: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatHeader {
    pub fn new(title: impl Into<String>, app: &AppContext) -> Self {
        Self {
            title: title.into(),
            app: app.clone(),
            sidebar_visible: false,
            on_toggle: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_sidebar_toggle<F: FnMut() + 'static>(
        mut self,
        sidebar_visible: bool,
        callback: F,
    ) -> Self {
        self.sidebar_visible = sidebar_visible;
        self.on_toggle = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self) {
        let app = &self.app;
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let title = Text::new(self.title.clone())
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(15.0)
            .finish();

        let mut row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(title);

        if let Some(cb) = self.on_toggle.clone() {
            let icon_name = if self.sidebar_visible {
                "left-panel-close"
            } else {
                "left-panel-open"
            };
            let toggle = TopbarButton::new(
                Icon::new(icon_name)
                    .with_size(18.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_active(self.sidebar_visible)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            row = row.with_child(toggle);
        }

        self.root = Some(
            Container::new(row.finish())
                .with_padding(EdgeInsets::uniform(spacing))
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .finish(),
        );
    }
}

impl Element for ChatHeader {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.app = app.clone();
        self.rebuild();
        let size = self
            .root
            .as_mut()
            .unwrap()
            .layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{EventContext, LayoutContext, PaintContext};
    use crate::event::DispatchedEvent;
    use crate::geometry::vec2f;

    #[test]
    fn chat_header_layouts_non_zero() {
        let app = AppContext::default();
        let mut header = ChatHeader::new("New chat", &app);
        let size = header.layout(
            SizeConstraint::loose(vec2f(600.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn chat_header_toggle_fires_callback() {
        let toggled = Rc::new(RefCell::new(false));
        let toggled_clone = toggled.clone();
        let app = AppContext::default();
        // Use an empty title so the toggle button is the first child and can be
        // hit with a predictable coordinate inside the header padding.
        let mut header = ChatHeader::new("", &app)
            .with_sidebar_toggle(false, move || *toggled_clone.borrow_mut() = true);

        header.layout(
            SizeConstraint::loose(vec2f(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        header.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(20.0, 20.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(20.0, 20.0),
            button: 0,
        };

        assert!(header.dispatch_event(&down, &mut event_ctx, &app));
        assert!(header.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*toggled.borrow());
    }
}
