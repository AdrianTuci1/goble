use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{DesktopState, ExecutionInfo};
use goble_ui::elements::{ActiveView, EdgeInsets, ShellState};
use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Element, EventContext,
    Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext, Point, Scrollable, SizeConstraint,
    Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::app::UiState;

fn execution_summary(exec: &ExecutionInfo) -> String {
    format!(
        "{} — agent: {} — status: {} — started: {} — finished: {}",
        &exec.id[..exec.id.len().min(8)],
        exec.agent_id.as_deref().unwrap_or("unknown"),
        exec.status,
        &exec.started_at[..exec.started_at.len().min(19)],
        exec.finished_at.as_deref().unwrap_or("running")
    )
}

pub struct ExecutionsViewPanel {
    content: Box<dyn Element>,
}

impl ExecutionsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        shell_state: Rc<RefCell<ShellState>>,
        ui_state: Rc<RefCell<UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        column = column.with_child(
            Text::new("Executions")
                .with_font_size(20.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );

        let mut executions = state.list_executions();
        executions.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        if executions.is_empty() {
            column = column.with_child(
                Text::new("No executions yet.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            for exec in executions {
                let exec_id = exec.id.clone();
                let summary = execution_summary(&exec);
                let state_for_trace = Arc::clone(&state);
                let shell_state_for_trace = Rc::clone(&shell_state);
                let ui_state_for_trace = Rc::clone(&ui_state);
                let dirty_for_trace = Rc::clone(&dirty);
                let row = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(summary)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        Button::new(
                            Text::new("Trace")
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || {
                            if state_for_trace.get_execution_trace(&exec_id).is_some() {
                                ui_state_for_trace.borrow_mut().selected_trace_id =
                                    Some(exec_id.clone());
                                shell_state_for_trace.borrow_mut().active_view =
                                    ActiveView::AgentTrace;
                                *dirty_for_trace.borrow_mut() = true;
                            } else {
                                log::warn!("execution trace not found for {}", exec_id);
                            }
                        })
                        .finish(),
                    )
                    .finish();
                column = column.with_child(
                    Container::new(row)
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(sm))
                        .finish(),
                );
            }
        }

        let content = Container::new(Scrollable::new(column.finish(), Axis::Vertical).finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        Self { content }
    }
}

impl Element for ExecutionsViewPanel {
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
