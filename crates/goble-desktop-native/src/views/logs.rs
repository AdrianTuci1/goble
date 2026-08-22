use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Fill, Flex, LayoutContext, PaintContext, Point, Scrollable, SizeConstraint, Text,
    TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct LogsViewPanel {
    content: Box<dyn Element>,
}

impl LogsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        _ui_state: Rc<RefCell<crate::app::UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let filter_state = Rc::new(RefCell::new(String::new()));
        let filter_for_change = Rc::clone(&filter_state);
        let filter_for_render = Rc::clone(&filter_state);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(
                Text::new("Logs")
                    .with_font_size(20.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                TextInput::new()
                    .with_placeholder("Filter logs...")
                    .with_on_change(move |v| *filter_for_change.borrow_mut() = v)
                    .finish(),
            );

        let logs = state.get_logs();
        let filter = filter_for_render.borrow().to_lowercase();
        let filtered: Vec<_> = logs
            .into_iter()
            .filter(|entry| {
                filter.is_empty()
                    || entry.message.to_lowercase().contains(&filter)
                    || entry.timestamp.to_lowercase().contains(&filter)
            })
            .collect();

        if filtered.is_empty() {
            column = column.with_child(
                Text::new("No logs match the current filter.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm);
            for entry in filtered.into_iter().rev().take(200) {
                let timestamp = &entry.timestamp[..entry.timestamp.len().min(19)];
                list = list.with_child(
                    Container::new(
                        Text::new(format!("{} {}", timestamp, entry.message))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
                );
            }
            column = column.with_child(
                Scrollable::new(list.finish(), goble_ui::elements::Axis::Vertical).finish(),
            );
        }

        let dirty_for_refresh = Rc::clone(&dirty);
        column = column.with_child(
            Button::new(
                Text::new("Refresh")
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                *dirty_for_refresh.borrow_mut() = true;
            })
            .finish(),
        );

        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        Self { content }
    }
}

impl Element for LogsViewPanel {
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
