use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Fill, Flex, Label, LabelSize, LayoutContext, MainAxisAlignment, PaintContext,
    Point, SizeConstraint, Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct AgentManagementView {
    content: Box<dyn Element>,
}

impl AgentManagementView {
    pub fn new(state: Arc<DesktopState>, dirty: Rc<RefCell<bool>>, app: &AppContext) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        column = column.with_child(
            Text::new("Agents")
                .with_font_size(20.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );

        let name_state = Rc::new(RefCell::new(String::new()));
        let prompt_state = Rc::new(RefCell::new(String::new()));
        let desc_state = Rc::new(RefCell::new(String::new()));

        let name_state_for_change = Rc::clone(&name_state);
        let prompt_state_for_change = Rc::clone(&prompt_state);
        let desc_state_for_change = Rc::clone(&desc_state);

        let name_input = TextInput::new()
            .with_placeholder("Agent name")
            .with_on_change(move |v| *name_state_for_change.borrow_mut() = v)
            .finish();
        let prompt_input = TextInput::new()
            .with_placeholder("Prompt")
            .with_on_change(move |v| *prompt_state_for_change.borrow_mut() = v)
            .finish();
        let desc_input = TextInput::new()
            .with_placeholder("Description")
            .with_on_change(move |v| *desc_state_for_change.borrow_mut() = v)
            .finish();

        let state_for_create = Arc::clone(&state);
        let dirty_for_create = Rc::clone(&dirty);
        let create = Button::new(Text::new("Create agent").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                let name = name_state.borrow().clone();
                let prompt = prompt_state.borrow().clone();
                let description = desc_state.borrow().clone();
                if name.is_empty() {
                    log::warn!("agent name is required");
                    return;
                }
                let description = if description.is_empty() { None } else { Some(description.as_str()) };
                if let Err(e) = state_for_create.create_agent(&name, &prompt, description, vec![]) {
                    log::error!("failed to create agent: {}", e);
                }
                *dirty_for_create.borrow_mut() = true;
            })
            .finish();

        column = column.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(name_input)
                    .with_child(prompt_input)
                    .with_child(desc_input)
                    .with_child(create)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );

        let agents = state.list_agents();
        if agents.is_empty() {
            column = column.with_child(
            Text::new("No agents yet. Create one above.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            for agent in agents {
                let state_for_run = Arc::clone(&state);
                let dirty_for_run = Rc::clone(&dirty);
                let agent_id = agent.id.clone();
                let name = agent.name.clone();
                let row = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(format!("{} — {}", name, &agent.id[..agent.id.len().min(8)]))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        Button::new(Text::new("Run").finish())
                            .with_variant(ButtonVariant::Primary)
                            .with_on_click(move || {
                                let worker_id = match state_for_run.resolve_worker_for_target("any", None, None) {
                                    Ok(wid) => wid,
                                    Err(e) => {
                                        log::error!("no worker available: {}", e);
                                        return;
                                    }
                                };
                let agent_id = goble_core::agent::AgentId(agent_id.clone());
                                if let Err(e) = state_for_run.run_agent(&worker_id, &agent_id, "Run from UI") {
                                    log::error!("failed to run agent: {}", e);
                                }
                                *dirty_for_run.borrow_mut() = true;
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

        column = column.with_child(
            Label::new("Recent runs")
                .with_size(LabelSize::Sm)
                .finish(),
        );

        let executions = state.list_executions();
        if executions.is_empty() {
            column = column.with_child(
                Text::new("No executions yet.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            for exec in executions {
                let line = format!(
                    "{} | agent: {} | worker: {} | status: {} | {} - {}",
                    &exec.id[..exec.id.len().min(8)],
                    exec.agent_id.as_deref().unwrap_or("unknown"),
                    exec.worker_id.as_deref().unwrap_or("local"),
                    exec.status,
                    &exec.started_at[..exec.started_at.len().min(19)],
                    exec.finished_at.as_deref().unwrap_or("running")
                );
                column = column.with_child(
                    Container::new(Text::new(line).with_theme_color(ColorToken::Text, app).finish())
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(sm))
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

impl Element for AgentManagementView {
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
