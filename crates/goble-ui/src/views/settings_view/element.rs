use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    Fill, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point,
    SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

use super::SettingsView;

impl SettingsView {
    pub(super) fn rebuild(&mut self, app: &AppContext, width: f32) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let nav_width = 160.0_f32;
        let pane_width = (width - nav_width - 1.0).max(200.0);

        let nav = self.build_nav(app);
        let pane = self.build_pane(app);
        let pane = crate::elements::ConstrainedBox::new(pane)
            .with_max_width(pane_width)
            .finish();

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                crate::elements::ConstrainedBox::new(nav)
                    .with_max_width(nav_width)
                    .finish(),
            )
            .with_child(Divider::vertical().finish())
            .with_child(pane);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(spacing);

        // Top-left Back button returns to the previous view.
        if let Some(cb) = self.on_back.clone() {
            let back_label = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
                .with_child(
                    Icon::new("chevron-left")
                        .with_size(16.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new("Back")
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .finish();
            let back = Button::new(back_label)
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
            column = column.with_child(
                Container::new(back)
                    .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Sm)))
                    .finish(),
            );
        }

        column = column.with_child(row.finish());

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Element for SettingsView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint.max.x);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
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
