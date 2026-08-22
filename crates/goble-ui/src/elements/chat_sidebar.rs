use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Icon, Label,
    LabelSize, LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Spacer, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub const CHAT_SIDEBAR_WIDTH: f32 = 260.0;

pub struct ChatSidebar {
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatSidebar {
    pub fn new(app: &AppContext) -> Self {
        let root = Self::build_root(app);
        Self {
            root,
            size: None,
            origin: None,
        }
    }

    fn build_root(app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let radius = app.theme.radius_px();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        // Header
        column = column.with_child(
            Text::new("Details")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(14.0)
                .finish(),
        );
        column = column.with_child(crate::elements::Divider::horizontal().finish());

        // Computer Use section
        column = column.with_child(
            Label::new("Computer Use")
                .with_size(LabelSize::Xs)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );

        let preview = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_spacing(xs)
                .with_child(
                    Icon::new("agentmode")
                        .with_size(32.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new("Active")
                        .with_theme_color(ColorToken::Success, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_border(app.theme.color(ColorToken::Border).into())
        .with_corner_radius(radius)
        .with_padding(EdgeInsets::uniform(spacing))
        .finish();
        column = column.with_child(preview);

        // Routines section
        column = column.with_child(
            Label::new("Routines")
                .with_size(LabelSize::Xs)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );

        let routines = vec![
            ("08:00", "Check email"),
            ("12:00", "Sync calendar"),
            ("18:00", "Generate daily summary"),
        ];
        let mut routines_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for (time, label) in routines {
            let row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Text::new(time)
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .finish(),
                )
                .with_child(
                    Text::new(label)
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .finish();
            routines_column = routines_column.with_child(
                Container::new(row)
                    .with_padding(EdgeInsets::uniform(sm))
                    .with_corner_radius(radius / 2.0)
                    .finish(),
            );
        }
        column = column.with_child(routines_column.finish());
        column = column.with_child(Spacer::new().finish());

        let inner = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        // Right-align the panel within the full-width row used for overlay layouts.
        Flex::row()
            .with_main_axis_size(crate::elements::MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Spacer::new().finish())
            .with_child(
                crate::elements::ConstrainedBox::new(inner)
                    .with_width(CHAT_SIDEBAR_WIDTH)
                    .finish(),
            )
            .finish()
    }
}

impl Element for ChatSidebar {
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn chat_sidebar_layouts() {
        let app = AppContext::default();
        let mut sidebar = ChatSidebar::new(&app);
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(400.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
