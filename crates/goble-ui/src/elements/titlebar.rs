use crate::color::ColorU;
use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, Element, EventContext,
    Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::style::EdgeInsets;
use crate::theme::ColorToken;

const TRAFFIC_LIGHT_SIZE: f32 = 12.0;
const TRAFFIC_LIGHT_GAP: f32 = 8.0;

pub struct TitleBar {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TitleBar {
    pub fn new(
        title: impl Into<String>,
        tabs: Vec<(impl Into<String>, bool, Box<dyn FnMut() + 'static>)>,
        tools: Vec<Box<dyn Element>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(crate::theme::SpacingToken::Md);
        let sm = app.theme.spacing_px(crate::theme::SpacingToken::Sm);

        let traffic = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(TRAFFIC_LIGHT_GAP)
            .with_child(traffic_light(ColorU::new(255, 95, 87, 255))) // close
            .with_child(traffic_light(ColorU::new(255, 189, 46, 255))) // minimize
            .with_child(traffic_light(ColorU::new(40, 200, 64, 255))) // maximize
            .finish();

        let title = Text::new(title)
            .with_font_size(14.0)
            .with_theme_color(ColorToken::Text, app)
            .finish();

        let mut tab_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for (label, active, callback) in tabs {
            let variant = if active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Ghost
            };
            tab_row = tab_row.with_child(
                Button::new(
                    Text::new(label)
                        .with_font_size(13.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_variant(variant)
                .with_on_click(callback)
                .finish(),
            );
        }

        let left = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(traffic)
            .with_child(title)
            .with_child(tab_row.finish())
            .finish();

        let mut tools_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for tool in tools {
            tools_row = tools_row.with_child(tool);
        }

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(left)
            .with_child(tools_row.finish())
            .finish();

        let root = Container::new(row)
            .with_padding(EdgeInsets::new(sm, spacing, sm, spacing))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .finish();

        Self {
            root,
            size: None,
            origin: None,
        }
    }
}

fn traffic_light(color: ColorU) -> Box<dyn Element> {
    // A small colored circle with a subtle border.
    struct Light {
        color: ColorU,
        size: Vector2F,
        origin: Option<Point>,
    }
    impl Element for Light {
        fn layout(
            &mut self,
            _constraint: SizeConstraint,
            _ctx: &mut LayoutContext,
            _app: &AppContext,
        ) -> Vector2F {
            self.size
        }
        fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
            self.origin = Some(Point::from_vec2f(origin, Default::default()));
            let rect = rectf(origin.x, origin.y, self.size.x, self.size.y);
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(rect, self.color, self.size.x * 0.5);
                renderer.stroke_rect(
                    rect,
                    ColorU::new(0, 0, 0, 30),
                    1.0,
                    self.size.x * 0.5,
                );
            }
        }
        fn size(&self) -> Option<Vector2F> {
            Some(self.size)
        }
        fn origin(&self) -> Option<Point> {
            self.origin
        }
    }
    Box::new(Light {
        color,
        size: vec2f(TRAFFIC_LIGHT_SIZE, TRAFFIC_LIGHT_SIZE),
        origin: None,
    })
}

impl Element for TitleBar {
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
