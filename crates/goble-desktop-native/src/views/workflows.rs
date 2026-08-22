use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, Element, EventContext, Fill, Flex, LayoutContext,
    PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct WorkflowsViewPanel {
    content: Box<dyn Element>,
}

impl WorkflowsViewPanel {
    pub fn new(
        _state: Arc<DesktopState>,
        _ui_state: Rc<RefCell<crate::app::UiState>>,
        _dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let content = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(
                    Text::new("Workflows")
                        .with_font_size(20.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_child(
                    Text::new("Workflow management coming soon.")
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_padding(goble_ui::elements::EdgeInsets::uniform(spacing))
        .finish();
        Self { content }
    }
}

impl Element for WorkflowsViewPanel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.content.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.content.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.content.size()
    }

    fn origin(&self) -> Option<Point> {
        self.content.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.content.dispatch_event(event, ctx, app)
    }
}
