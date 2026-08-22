use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    ActiveView, AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets,
    Element, EventContext, Fill, Flex, LayoutContext, PaintContext, Point, Scrollable, ShellState,
    SizeConstraint, Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct SearchViewPanel {
    content: Box<dyn Element>,
}

impl SearchViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        ui_state: Rc<RefCell<crate::app::UiState>>,
        shell_state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let query_state = Rc::new(RefCell::new(String::new()));
        let query_for_change = Rc::clone(&query_state);
        let query_for_search = Rc::clone(&query_state);
        let state_for_search = Arc::clone(&state);
        let dirty_for_search = Rc::clone(&dirty);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(
                Text::new("Search")
                    .with_font_size(20.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Search chats and executions...")
                            .with_on_change(move |v| *query_for_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        Button::new(Text::new("Search").with_theme_color(ColorToken::Text, app).finish())
                            .with_variant(ButtonVariant::Primary)
                            .with_on_click(move || {
                                let _ = query_for_search.borrow().clone();
                                let _ = state_for_search.list_chats();
                                let _ = state_for_search.list_executions();
                                *dirty_for_search.borrow_mut() = true;
                            })
                            .finish(),
                    )
                    .finish(),
            );

        let query = query_state.borrow().to_lowercase();
        if query.is_empty() {
            column = column.with_child(
                Text::new("Enter a query to search.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            let mut results = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm);
            let mut any = false;

            let chats = state.list_chats();
            for chat in chats {
                let preview = state
                    .list_chat_messages(&chat.id)
                    .ok()
                    .and_then(|msgs| msgs.last().map(|m| m.content.clone()))
                    .unwrap_or_default();
                if chat.title.to_lowercase().contains(&query)
                    || preview.to_lowercase().contains(&query)
                {
                    any = true;
                    let chat_id = chat.id.clone();
                    let ui_state_for_nav = Rc::clone(&ui_state);
                    let shell_state_for_nav = Rc::clone(&shell_state);
                    let dirty_for_nav = Rc::clone(&dirty);
                    results = results.with_child(
                        Button::new(
                            Text::new(format!("Chat: {} - {}", chat.title, preview.chars().take(40).collect::<String>()))
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Ghost)
                        .with_on_click(move || {
                            ui_state_for_nav.borrow_mut().selected_chat_id = Some(chat_id.clone());
                            shell_state_for_nav.borrow_mut().active_view = ActiveView::Chat;
                            *dirty_for_nav.borrow_mut() = true;
                        })
                        .finish(),
                    );
                }
            }

            let executions = state.list_executions();
            for exec in executions {
                let state_for_exec_nav = Arc::clone(&state);
                let text = format!(
                    "{} {} {}",
                    exec.id,
                    exec.status,
                    exec.started_at
                )
                .to_lowercase();
                if text.contains(&query) {
                    any = true;
                    let exec_id = exec.id.clone();
                    let ui_state_for_nav = Rc::clone(&ui_state);
                    let shell_state_for_nav = Rc::clone(&shell_state);
                    let dirty_for_nav = Rc::clone(&dirty);
                    results = results.with_child(
                        Button::new(
                            Text::new(format!(
                                "Execution: {} - {} - {}",
                                &exec.id[..exec.id.len().min(8)],
                                exec.status,
                                &exec.started_at[..exec.started_at.len().min(19)]
                            ))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                        )
                        .with_variant(ButtonVariant::Ghost)
                        .with_on_click(move || {
                            if state_for_exec_nav.get_execution_trace(&exec_id).is_some() {
                                ui_state_for_nav.borrow_mut().selected_trace_id = Some(exec_id.clone());
                                shell_state_for_nav.borrow_mut().active_view = ActiveView::AgentTrace;
                                *dirty_for_nav.borrow_mut() = true;
                            }
                        })
                        .finish(),
                    );
                }
            }

            if any {
                column = column.with_child(Scrollable::new(results.finish(), goble_ui::elements::Axis::Vertical).finish());
            } else {
                column = column.with_child(
                    Text::new("No results found.")
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
        }

        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        Self { content }
    }
}

impl Element for SearchViewPanel {
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
