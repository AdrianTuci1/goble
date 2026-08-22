use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::AgentId;
use goble_core::workflow::{WorkflowStep, WorkflowId};
use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Checkbox, Container, CrossAxisAlignment, EdgeInsets,
    Element, EventContext, Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext, Point,
    Scrollable, SizeConstraint, Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct WorkflowsViewPanel {
    content: Box<dyn Element>,
}

impl WorkflowsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        _ui_state: Rc<RefCell<crate::app::UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let agents = state.list_agents();
        let _agent_options: Vec<(String, String)> = agents
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(
                Text::new("Workflows")
                    .with_font_size(20.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );

        // Create workflow form.
        let wf_name = Rc::new(RefCell::new(String::new()));
        let wf_desc = Rc::new(RefCell::new(String::new()));
        let wf_steps = Rc::new(RefCell::new(String::new()));
        let wf_cron = Rc::new(RefCell::new(String::new()));
        let wf_name_change = Rc::clone(&wf_name);
        let wf_desc_change = Rc::clone(&wf_desc);
        let wf_steps_change = Rc::clone(&wf_steps);
        let wf_cron_change = Rc::clone(&wf_cron);
        let state_for_create = Arc::clone(&state);
        let dirty_for_create = Rc::clone(&dirty);

        column = column.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(
                        Text::new("Create workflow")
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Name")
                            .with_on_change(move |v| *wf_name_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Description")
                            .with_on_change(move |v| *wf_desc_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Steps: name:agent_id,name:agent_id")
                            .with_on_change(move |v| *wf_steps_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Cron expression (optional)")
                            .with_on_change(move |v| *wf_cron_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        Button::new(Text::new("Create").with_theme_color(ColorToken::Text, app).finish())
                            .with_variant(ButtonVariant::Primary)
                            .with_on_click(move || {
                                let name = wf_name.borrow().clone();
                                let description = wf_desc.borrow().clone();
                                let steps_text = wf_steps.borrow().clone();
                                let cron = wf_cron.borrow().clone();
                                if name.is_empty() {
                                    log::warn!("workflow name is required");
                                    return;
                                }
                                let mut steps = Vec::new();
                                for (idx, part) in steps_text.split(',').enumerate() {
                                    let mut it = part.split(':');
                                    let step_name = it
                                        .next()
                                        .map(|s| s.trim().to_string())
                                        .unwrap_or_else(|| format!("step-{}", idx));
                                    let agent_id = it
                                        .next()
                                        .map(|s| s.trim().to_string())
                                        .unwrap_or_default();
                                    if agent_id.is_empty() {
                                        log::warn!("step {} missing agent id", step_name);
                                        continue;
                                    }
                                    steps.push(WorkflowStep {
                                        id: format!("{}-{}", step_name, idx),
                                        name: step_name,
                                        agent_id: AgentId(agent_id),
                                        input_template: String::new(),
                                        depends_on: Vec::new(),
                                    });
                                }
                                let trigger = if cron.is_empty() {
                                    goble_core::agent::Trigger::Manual
                                } else {
                                    goble_core::agent::Trigger::Cron { expression: cron }
                                };
                                if let Err(e) = state_for_create.create_workflow(
                                    &name,
                                    &description,
                                    steps,
                                    trigger,
                                ) {
                                    log::error!("failed to create workflow: {}", e);
                                }
                                *dirty_for_create.borrow_mut() = true;
                            })
                            .finish(),
                    )
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );

        let workflows = state.list_workflows();
        if workflows.is_empty() {
            column = column.with_child(
                Text::new("No workflows yet.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm);
            for wf in workflows {
                let wf_id = wf.id.clone();
                let title = wf.name.clone();
                let description = wf.description.clone();
                let enabled = wf.enabled;
                let trigger_label = match &wf.trigger {
                    goble_core::agent::Trigger::Manual => "manual".to_string(),
                    goble_core::agent::Trigger::Cron { expression } => format!("cron({})", expression),
                    goble_core::agent::Trigger::Http { path } => format!("http({})", path),
                    goble_core::agent::Trigger::Heartbeat { interval_seconds } => format!("heartbeat({}s)", interval_seconds),
                };
                let _tags: Vec<String> = wf
                    .steps
                    .iter()
                    .map(|s| s.name.clone())
                    .chain(std::iter::once(trigger_label))
                    .collect();

                let state_for_delete = Arc::clone(&state);
                let dirty_for_delete = Rc::clone(&dirty);
                let id_for_delete = wf_id.clone();
                let delete = Button::new(Text::new("Delete").with_theme_color(ColorToken::Text, app).finish())
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || {
                        if let Err(e) = state_for_delete.delete_workflow(&WorkflowId(id_for_delete.clone())) {
                            log::error!("failed to delete workflow: {}", e);
                        }
                        *dirty_for_delete.borrow_mut() = true;
                    })
                    .finish();

                let state_for_toggle = Arc::clone(&state);
                let dirty_for_toggle = Rc::clone(&dirty);
                let id_for_toggle = wf_id.clone();
                let toggle = Checkbox::new()
                    .with_label(Text::new(if enabled { "Enabled" } else { "Disabled" }).with_theme_color(ColorToken::Text, app).finish())
                    .with_checked(enabled)
                    .with_on_change(move |checked| {
                        if let Some(info) = state_for_toggle
                            .list_workflows()
                            .into_iter()
                            .find(|w| w.id == id_for_toggle)
                        {
                            let mut spec = goble_core::workflow::Workflow::new(&info.name, &info.description)
                                .with_trigger(info.trigger.clone());
                            for step in &info.steps {
                                spec = spec.with_step(step.clone());
                            }
                            spec.enabled = checked;
                            let spec_json = match serde_json::to_string(&spec) {
                                Ok(j) => j,
                                Err(e) => {
                                    log::error!("failed to serialize workflow: {}", e);
                                    return;
                                }
                            };
                            let trigger_json = serde_json::to_string(&spec.trigger).unwrap_or_default();
                            let now = chrono::Utc::now().to_rfc3339();
                            if let Err(e) = state_for_toggle.store_clone().insert_workflow(
                                &id_for_toggle,
                                &spec.name,
                                &spec.description,
                                &spec_json,
                                &trigger_json,
                                spec.enabled,
                                &info.created_at,
                                &now,
                            ) {
                                log::error!("failed to toggle workflow: {}", e);
                            }
                        }
                        *dirty_for_toggle.borrow_mut() = true;
                    })
                    .finish();

                let header = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(title)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(delete)
                    .finish();

                let mut body = Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(header);
                if !description.is_empty() {
                    body = body.with_child(
                        Text::new(description)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                }
                let mut steps_text = Vec::new();
                for step in &wf.steps {
                    steps_text.push(format!(
                        "{} -> agent {}",
                        step.name,
                        step.agent_id.0.chars().take(8).collect::<String>()
                    ));
                }
                if !steps_text.is_empty() {
                    body = body.with_child(
                        Text::new(steps_text.join(", "))
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                }
                body = body.with_child(toggle);

                list = list.with_child(
                    Container::new(body.finish())
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(sm))
                        .finish(),
                );
            }
            column = column.with_child(Scrollable::new(list.finish(), goble_ui::elements::Axis::Vertical).finish());
        }

        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(spacing))
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
