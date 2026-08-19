use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Chip, Container, CrossAxisAlignment, Element, EventContext, Fill, Flex,
    LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct AgentCard {
    root: Box<dyn Element>,
    state: InteractiveState,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AgentCard {
    pub fn new(
        avatar: Box<dyn Element>,
        name: impl Into<String>,
        description: impl Into<String>,
        tags: impl IntoIterator<Item = impl Into<String>>,
        app: &AppContext,
    ) -> Self {
        let root = Self::build_root(avatar, name, description, tags, app);
        Self {
            root,
            state: InteractiveState::default(),
            on_click: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_root(
        avatar: Box<dyn Element>,
        name: impl Into<String>,
        description: impl Into<String>,
        tags: impl IntoIterator<Item = impl Into<String>>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let spacing_md = app.theme.spacing_px(SpacingToken::Md);
        let spacing_sm = app.theme.spacing_px(SpacingToken::Sm);
        let radius = app.theme.radius_px();

        let name = Text::new(name)
            .with_theme_color(ColorToken::Text, app)
            .finish();
        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing_md)
            .with_child(avatar)
            .with_child(name)
            .finish();

        let description = Text::new(description)
            .with_theme_color(ColorToken::Muted, app)
            .finish();

        let mut tag_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing_sm);
        for tag in tags {
            let chip = Chip::new(Text::new(tag).with_font_size(11.0).finish()).finish();
            tag_row = tag_row.with_child(chip);
        }

        let body = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(spacing_md)
            .with_child(header)
            .with_child(description)
            .with_child(tag_row.finish())
            .finish();

        Container::new(body)
            .with_padding(crate::style::EdgeInsets::uniform(spacing_md))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .with_corner_radius(radius)
            .finish()
    }
}

impl Element for AgentCard {
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
    use crate::elements::Avatar;
    use crate::geometry::vec2f;

    #[test]
    fn agent_card_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let app = AppContext::default();
        let mut card = AgentCard::new(
            Avatar::new("A").finish(),
            "Coder",
            "Writes and reviews code.",
            ["rust", "review"],
            &app,
        )
        .with_on_click(move || *clicked_clone.borrow_mut() = true);

        card.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        card.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(card.dispatch_event(&down, &mut event_ctx, &app));
        assert!(card.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }
}
