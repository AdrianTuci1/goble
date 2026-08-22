use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::{AgentId, Trigger};
use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AgentCard, AppContext, Avatar, Button, ButtonVariant, Checkbox, Container, CrossAxisAlignment,
    EdgeInsets, Element, EventContext, Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext,
    Point, RightPanel, Scrollable, SizeConstraint, Text, TextArea, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::app::UiState;

pub struct AgentManagementView {
    content: Box<dyn Element>,
}

fn action_button(
    label: &str,
    on_click: impl FnMut() + 'static,
    app: &AppContext,
) -> Box<dyn Element> {
    Button::new(
        Text::new(label)
            .with_theme_color(ColorToken::Text, app)
            .finish(),
    )
    .with_variant(ButtonVariant::Ghost)
    .with_on_click(on_click)
    .finish()
}

impl AgentManagementView {
    pub fn new(
        state: Arc<DesktopState>,
        ui_state: Rc<RefCell<UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let agents = state.list_agents();
        let selected_id = ui_state.borrow().selected_agent_id.clone();
        let selected_agent = selected_id
            .as_ref()
            .and_then(|id| agents.iter().find(|a| &a.id == id).cloned());

        let mcp_servers = state.list_mcp_servers().unwrap_or_default();
        let mut discovered_tools: Vec<String> = Vec::new();
        for server in &mcp_servers {
            for tool in &server.discovered_tools {
                if !discovered_tools.contains(tool) {
                    discovered_tools.push(tool.clone());
                }
            }
        }

        // Left column: header, new-agent form, agent cards.
        let mut left = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let new_open = ui_state.borrow().agent_new_open;
        let ui_state_for_new_toggle = Rc::clone(&ui_state);
        let dirty_for_new_toggle = Rc::clone(&dirty);
        let header = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new("Agents")
                    .with_font_size(20.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                Button::new(
                    Text::new("New agent")
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_variant(ButtonVariant::Primary)
                .with_on_click(move || {
                    ui_state_for_new_toggle.borrow_mut().agent_new_open = !new_open;
                    *dirty_for_new_toggle.borrow_mut() = true;
                })
                .finish(),
            )
            .finish();
        left = left.with_child(header);

        if new_open {
            let name_state = Rc::new(RefCell::new(String::new()));
            let prompt_state = Rc::new(RefCell::new(String::new()));
            let desc_state = Rc::new(RefCell::new(String::new()));
            let name_state_for_change = Rc::clone(&name_state);
            let prompt_state_for_change = Rc::clone(&prompt_state);
            let desc_state_for_change = Rc::clone(&desc_state);

            let state_for_create = Arc::clone(&state);
            let dirty_for_create = Rc::clone(&dirty);
            let ui_state_for_create = Rc::clone(&ui_state);
            let create = Button::new(
                Text::new("Create")
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                let name = name_state.borrow().clone();
                let prompt = prompt_state.borrow().clone();
                let description = desc_state.borrow().clone();
                if name.is_empty() {
                    log::warn!("agent name is required");
                    return;
                }
                let description = if description.is_empty() {
                    None
                } else {
                    Some(description)
                };
                match state_for_create.create_agent(&name, &prompt, description.as_deref(), vec![])
                {
                    Ok(info) => {
                        ui_state_for_create.borrow_mut().selected_agent_id = Some(info.id);
                        ui_state_for_create.borrow_mut().agent_new_open = false;
                    }
                    Err(e) => log::error!("failed to create agent: {}", e),
                }
                *dirty_for_create.borrow_mut() = true;
            })
            .finish();

            let form = Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Agent name")
                            .with_on_change(move |v| *name_state_for_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextArea::new()
                            .with_placeholder("Prompt")
                            .with_on_change(move |v| *prompt_state_for_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Description")
                            .with_on_change(move |v| *desc_state_for_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(create)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish();
            left = left.with_child(form);
        }

        if agents.is_empty() {
            left = left.with_child(
                Text::new("No agents yet. Create one above.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }

        let mut list = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);

        for agent in agents {
            let agent_id = agent.id.clone();
            let name = agent.name.clone();
            let description = agent.spec.description.clone();
            let mut tags: Vec<String> = agent.spec.tools.clone();
            for t in &agent.spec.triggers {
                tags.push(match t {
                    Trigger::Manual => "manual".to_string(),
                    Trigger::Cron { .. } => "cron".to_string(),
                    Trigger::Http { .. } => "http".to_string(),
                    Trigger::Heartbeat { .. } => "heartbeat".to_string(),
                });
            }

            let avatar = Avatar::new(&name)
                .with_theme_background(ColorToken::Accent, app)
                .with_theme_foreground(ColorToken::Text, app)
                .finish();

            let state_for_select = Arc::clone(&state);
            let ui_state_for_select = Rc::clone(&ui_state);
            let dirty_for_select = Rc::clone(&dirty);
            let select_id = agent_id.clone();
            let card = AgentCard::new(avatar, name.clone(), description.clone(), tags, app)
                .with_on_click(move || {
                    ui_state_for_select.borrow_mut().selected_agent_id = Some(select_id.clone());
                    ui_state_for_select.borrow_mut().agent_editing = false;
                    ui_state_for_select.borrow_mut().agent_scheduling = false;
                    *dirty_for_select.borrow_mut() = true;
                })
                .finish();

            let run_id = agent_id.clone();
            let state_for_run = Arc::clone(&state);
            let dirty_for_run = Rc::clone(&dirty);
            let run = action_button(
                "Run",
                move || {
                    let worker_id = match state_for_run.resolve_worker_for_target("any", None, None)
                    {
                        Ok(wid) => wid,
                        Err(e) => {
                            log::error!("no worker available: {}", e);
                            return;
                        }
                    };
                    let id = AgentId(run_id.clone());
                    if let Err(e) = state_for_run.run_agent(&worker_id, &id, "Run from UI") {
                        log::error!("failed to run agent: {}", e);
                    }
                    *dirty_for_run.borrow_mut() = true;
                },
                app,
            );

            let schedule_id = agent_id.clone();
            let ui_state_for_schedule = Rc::clone(&ui_state);
            let dirty_for_schedule = Rc::clone(&dirty);
            let schedule = action_button(
                "Schedule",
                move || {
                    ui_state_for_schedule.borrow_mut().selected_agent_id =
                        Some(schedule_id.clone());
                    ui_state_for_schedule.borrow_mut().agent_scheduling = true;
                    ui_state_for_schedule.borrow_mut().agent_editing = false;
                    *dirty_for_schedule.borrow_mut() = true;
                },
                app,
            );

            let edit_id = agent_id.clone();
            let ui_state_for_edit = Rc::clone(&ui_state);
            let dirty_for_edit = Rc::clone(&dirty);
            let edit = action_button(
                "Edit",
                move || {
                    ui_state_for_edit.borrow_mut().selected_agent_id = Some(edit_id.clone());
                    ui_state_for_edit.borrow_mut().agent_editing = true;
                    ui_state_for_edit.borrow_mut().agent_scheduling = false;
                    if let Some(a) = state_for_select
                        .list_agents()
                        .into_iter()
                        .find(|a| a.id == edit_id)
                    {
                        let mut u = ui_state_for_edit.borrow_mut();
                        u.agent_edit_name = a.name.clone();
                        u.agent_edit_prompt = a.spec.prompt.clone();
                        u.agent_edit_description = a.spec.description.clone();
                        u.agent_edit_tools = a.spec.tools.clone();
                        u.agent_edit_mcp_ids = a.spec.mcp_ids.clone();
                    }
                    *dirty_for_edit.borrow_mut() = true;
                },
                app,
            );

            let delete_id = agent_id.clone();
            let state_for_delete = Arc::clone(&state);
            let dirty_for_delete = Rc::clone(&dirty);
            let ui_state_for_delete = Rc::clone(&ui_state);
            let delete = action_button(
                "Delete",
                move || {
                    if let Err(e) = state_for_delete.delete_agent(&AgentId(delete_id.clone())) {
                        log::error!("failed to delete agent: {}", e);
                    }
                    ui_state_for_delete.borrow_mut().selected_agent_id = None;
                    *dirty_for_delete.borrow_mut() = true;
                },
                app,
            );

            let actions = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(run)
                .with_child(schedule)
                .with_child(edit)
                .with_child(delete)
                .finish();

            let tile = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm)
                .with_child(card)
                .with_child(actions)
                .finish();

            list = list.with_child(
                Container::new(tile)
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
            );
        }

        let list_scroll =
            Scrollable::new(list.finish(), goble_ui::elements::Axis::Vertical).finish();
        left = left.with_child(list_scroll);

        // Right panel: details / edit / schedule for selected agent.
        let mut right_children: Vec<Box<dyn Element>> = Vec::new();
        right_children.push(
            Text::new("Agent details")
                .with_font_size(18.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );

        match selected_agent {
            Some(agent) => {
                if ui_state.borrow().agent_scheduling {
                    right_children.push(
                        Text::new(format!("Schedule: {}", agent.name))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    );
                    let cron_value = ui_state.borrow().agent_schedule_cron.clone();
                    let cron_state = Rc::new(RefCell::new(cron_value.clone()));
                    let cron_for_change = Rc::clone(&cron_state);
                    let ui_state_for_cron = Rc::clone(&ui_state);
                    right_children.push(
                        TextInput::new()
                            .with_placeholder("* * * * *")
                            .with_value(cron_value)
                            .with_on_change(move |v| {
                                *cron_for_change.borrow_mut() = v.clone();
                                ui_state_for_cron.borrow_mut().agent_schedule_cron = v;
                            })
                            .finish(),
                    );
                    let state_for_save = Arc::clone(&state);
                    let dirty_for_save = Rc::clone(&dirty);
                    let ui_state_for_save = Rc::clone(&ui_state);
                    let agent_id = AgentId(agent.id.clone());
                    right_children.push(
                        Button::new(
                            Text::new("Save schedule")
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || {
                            let expression = cron_state.borrow().clone();
                            if expression.is_empty() {
                                log::warn!("cron expression is required");
                                return;
                            }
                            let worker_id =
                                match state_for_save.resolve_worker_for_target("any", None, None) {
                                    Ok(wid) => wid,
                                    Err(e) => {
                                        log::error!("no worker available: {}", e);
                                        return;
                                    }
                                };
                            let trigger = Trigger::Cron { expression };
                            if let Err(e) =
                                state_for_save.schedule_agent(&worker_id, &agent_id, trigger)
                            {
                                log::error!("failed to schedule agent: {}", e);
                            }
                            ui_state_for_save.borrow_mut().agent_scheduling = false;
                            *dirty_for_save.borrow_mut() = true;
                        })
                        .finish(),
                    );
                } else if ui_state.borrow().agent_editing {
                    right_children.push(
                        Text::new(format!("Edit: {}", agent.name))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    );
                    let edit_name = ui_state.borrow().agent_edit_name.clone();
                    let edit_prompt = ui_state.borrow().agent_edit_prompt.clone();
                    let edit_description = ui_state.borrow().agent_edit_description.clone();
                    let name_state = Rc::new(RefCell::new(edit_name.clone()));
                    let prompt_state = Rc::new(RefCell::new(edit_prompt.clone()));
                    let desc_state = Rc::new(RefCell::new(edit_description.clone()));
                    let name_for_change = Rc::clone(&name_state);
                    let prompt_for_change = Rc::clone(&prompt_state);
                    let desc_for_change = Rc::clone(&desc_state);
                    let ui_state_for_name = Rc::clone(&ui_state);
                    let ui_state_for_prompt = Rc::clone(&ui_state);
                    let ui_state_for_desc = Rc::clone(&ui_state);

                    right_children.push(
                        TextInput::new()
                            .with_value(edit_name)
                            .with_placeholder("Name")
                            .with_on_change(move |v| {
                                *name_for_change.borrow_mut() = v.clone();
                                ui_state_for_name.borrow_mut().agent_edit_name = v;
                            })
                            .finish(),
                    );
                    right_children.push(
                        TextArea::new()
                            .with_value(edit_prompt)
                            .with_placeholder("Prompt")
                            .with_on_change(move |v| {
                                *prompt_for_change.borrow_mut() = v.clone();
                                ui_state_for_prompt.borrow_mut().agent_edit_prompt = v;
                            })
                            .finish(),
                    );
                    right_children.push(
                        TextInput::new()
                            .with_value(edit_description)
                            .with_placeholder("Description")
                            .with_on_change(move |v| {
                                *desc_for_change.borrow_mut() = v.clone();
                                ui_state_for_desc.borrow_mut().agent_edit_description = v;
                            })
                            .finish(),
                    );

                    // Tool selection.
                    if !discovered_tools.is_empty() {
                        right_children.push(
                            Text::new("Tools")
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        );
                        for tool in &discovered_tools {
                            let tool = tool.clone();
                            let checked = ui_state.borrow().agent_edit_tools.contains(&tool);
                            let ui_state_for_tool = Rc::clone(&ui_state);
                            let dirty_for_tool = Rc::clone(&dirty);
                            right_children.push(
                                Checkbox::new()
                                    .with_label(
                                        Text::new(tool.clone())
                                            .with_theme_color(ColorToken::Text, app)
                                            .finish(),
                                    )
                                    .with_checked(checked)
                                    .with_on_change(move |enabled| {
                                        let mut u = ui_state_for_tool.borrow_mut();
                                        if enabled {
                                            if !u.agent_edit_tools.contains(&tool) {
                                                u.agent_edit_tools.push(tool.clone());
                                            }
                                        } else {
                                            u.agent_edit_tools.retain(|t| t != &tool);
                                        }
                                        drop(u);
                                        *dirty_for_tool.borrow_mut() = true;
                                    })
                                    .finish(),
                            );
                        }
                    }

                    // MCP selection.
                    if !mcp_servers.is_empty() {
                        right_children.push(
                            Text::new("MCP servers")
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        );
                        for server in &mcp_servers {
                            let server_id = server.id.clone();
                            let checked = ui_state.borrow().agent_edit_mcp_ids.contains(&server_id);
                            let ui_state_for_mcp = Rc::clone(&ui_state);
                            let dirty_for_mcp = Rc::clone(&dirty);
                            right_children.push(
                                Checkbox::new()
                                    .with_label(
                                        Text::new(format!("{}", server.name))
                                            .with_theme_color(ColorToken::Text, app)
                                            .finish(),
                                    )
                                    .with_checked(checked)
                                    .with_on_change(move |enabled| {
                                        let mut u = ui_state_for_mcp.borrow_mut();
                                        if enabled {
                                            if !u.agent_edit_mcp_ids.contains(&server_id) {
                                                u.agent_edit_mcp_ids.push(server_id.clone());
                                            }
                                        } else {
                                            u.agent_edit_mcp_ids.retain(|id| id != &server_id);
                                        }
                                        drop(u);
                                        *dirty_for_mcp.borrow_mut() = true;
                                    })
                                    .finish(),
                            );
                        }
                    }

                    let state_for_update = Arc::clone(&state);
                    let dirty_for_update = Rc::clone(&dirty);
                    let ui_state_for_update = Rc::clone(&ui_state);
                    let agent_id = AgentId(agent.id.clone());
                    right_children.push(
                        Button::new(
                            Text::new("Save")
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || {
                            let u = ui_state_for_update.borrow();
                            let name = u.agent_edit_name.clone();
                            let prompt = u.agent_edit_prompt.clone();
                            let description = u.agent_edit_description.clone();
                            let tools = u.agent_edit_tools.clone();
                            let mcp_ids = u.agent_edit_mcp_ids.clone();
                            drop(u);
                            if name.is_empty() {
                                log::warn!("agent name is required");
                                return;
                            }
                            if let Err(e) = state_for_update.update_agent(
                                &agent_id,
                                &name,
                                &prompt,
                                if description.is_empty() {
                                    None
                                } else {
                                    Some(&description)
                                },
                                tools,
                            ) {
                                log::error!("failed to update agent: {}", e);
                                return;
                            }
                            // Also update mcp_ids by reloading spec and rewriting.
                            if let Some(info) = state_for_update
                                .list_agents()
                                .into_iter()
                                .find(|a| a.id == agent_id.0)
                            {
                                let mut spec = info.spec.clone();
                                spec.mcp_ids = mcp_ids;
                                let now = chrono::Utc::now().to_rfc3339();
                                let spec_json = match serde_json::to_string(&spec) {
                                    Ok(j) => j,
                                    Err(e) => {
                                        log::error!("failed to serialize agent spec: {}", e);
                                        return;
                                    }
                                };
                                if let Err(e) = state_for_update.store_clone().update_agent(
                                    &agent_id.0,
                                    &spec.name,
                                    &spec_json,
                                    &now,
                                ) {
                                    log::error!("failed to persist mcp ids: {}", e);
                                }
                            }
                            ui_state_for_update.borrow_mut().agent_editing = false;
                            *dirty_for_update.borrow_mut() = true;
                        })
                        .finish(),
                    );
                } else {
                    right_children.push(
                        Text::new(agent.name.clone())
                            .with_font_size(16.0)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    );
                    if !agent.spec.description.is_empty() {
                        right_children.push(
                            Text::new(agent.spec.description.clone())
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        );
                    }
                    right_children.push(
                        Text::new(format!("Prompt: {}", agent.spec.prompt))
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                    if !agent.spec.tools.is_empty() {
                        right_children.push(
                            Text::new(format!("Tools: {}", agent.spec.tools.join(", ")))
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        );
                    }
                    if !agent.spec.mcp_ids.is_empty() {
                        right_children.push(
                            Text::new(format!("MCP: {}", agent.spec.mcp_ids.join(", ")))
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        );
                    }
                    let triggers_text = agent
                        .spec
                        .triggers
                        .iter()
                        .map(|t| match t {
                            Trigger::Manual => "manual".to_string(),
                            Trigger::Cron { expression } => format!("cron({})", expression),
                            Trigger::Http { path } => format!("http({})", path),
                            Trigger::Heartbeat { interval_seconds } => {
                                format!("heartbeat({}s)", interval_seconds)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    right_children.push(
                        Text::new(format!("Triggers: {}", triggers_text))
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                }
            }
            None => {
                right_children.push(
                    Text::new("Select an agent to view details.")
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
        }

        let right = RightPanel::new(right_children, 280.0, app);

        let body = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(left.finish())
            .with_child(right.finish())
            .finish();

        let content = Container::new(body)
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
