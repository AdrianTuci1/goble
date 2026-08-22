use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Fill, Flex, Label, LabelSize, LayoutContext, PaintContext, Point, SizeConstraint,
    Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct DriveViewPanel {
    content: Box<dyn Element>,
}

impl DriveViewPanel {
    pub fn new(state: Arc<DesktopState>, dirty: Rc<RefCell<bool>>, app: &AppContext) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        column = column.with_child(
            Text::new("Drive")
                .with_font_size(20.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        );

        // Workflows section with create form
        column = column.with_child(Label::new("Workflows").with_size(LabelSize::Sm).finish());

        let wf_name_state = Rc::new(RefCell::new(String::new()));
        let wf_desc_state = Rc::new(RefCell::new(String::new()));
        let wf_name_input = TextInput::new()
            .with_placeholder("Workflow name")
            .with_on_change({
                let state = Rc::clone(&wf_name_state);
                move |v| *state.borrow_mut() = v
            })
            .finish();
        let wf_desc_input = TextInput::new()
            .with_placeholder("Description")
            .with_on_change({
                let state = Rc::clone(&wf_desc_state);
                move |v| *state.borrow_mut() = v
            })
            .finish();
        let state_for_wf = Arc::clone(&state);
        let dirty_for_wf = Rc::clone(&dirty);
        let wf_create = Button::new(Text::new("Create workflow").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                let name = wf_name_state.borrow().clone();
                let description = wf_desc_state.borrow().clone();
                if name.is_empty() {
                    log::warn!("workflow name is required");
                    return;
                }
                if let Err(e) = state_for_wf.create_workflow(
                    &name,
                    &description,
                    vec![],
                    goble_core::agent::Trigger::Manual,
                ) {
                    log::error!("failed to create workflow: {}", e);
                }
                *dirty_for_wf.borrow_mut() = true;
            })
            .finish();
        column = column.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(wf_name_input)
                    .with_child(wf_desc_input)
                    .with_child(wf_create)
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
            for wf in workflows {
                let line = format!(
                    "{} | {} | {} | {}",
                    &wf.id[..wf.id.len().min(8)],
                    wf.name,
                    wf.description,
                    if wf.enabled { "enabled" } else { "disabled" }
                );
                column = column.with_child(
                    Container::new(
                        Text::new(line)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
                );
            }
        }

        column = column.with_child(Label::new("Agents").with_size(LabelSize::Sm).finish());
        let agents = state.list_agents();
        if agents.is_empty() {
            column = column.with_child(
                Text::new("No agents yet.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            for agent in agents {
                let line = format!("{} | {}", &agent.id[..agent.id.len().min(8)], agent.name);
                column = column.with_child(
                    Container::new(
                        Text::new(line)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
                );
            }
        }

        column = column.with_child(Label::new("Teams").with_size(LabelSize::Sm).finish());

        let team_name_state = Rc::new(RefCell::new(String::new()));
        let team_name_input = TextInput::new()
            .with_placeholder("Team name")
            .with_on_change({
                let state = Rc::clone(&team_name_state);
                move |v| *state.borrow_mut() = v
            })
            .finish();
        let state_for_team = Arc::clone(&state);
        let dirty_for_team = Rc::clone(&dirty);
        let team_create = Button::new(Text::new("Create team").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                let name = team_name_state.borrow().clone();
                if name.is_empty() {
                    log::warn!("team name is required");
                    return;
                }
                let id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = state_for_team.create_team(&id, &name, "{}", vec![]) {
                    log::error!("failed to create team: {}", e);
                }
                *dirty_for_team.borrow_mut() = true;
            })
            .finish();
        column = column.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(team_name_input)
                    .with_child(team_create)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );

        let teams = state.list_teams();
        if teams.is_empty() {
            column = column.with_child(
                Text::new("No teams yet.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            for team in teams {
                let line = format!(
                    "{} | {} | members: {}",
                    &team.id[..team.id.len().min(8)],
                    team.name,
                    team.members.len()
                );
                column = column.with_child(
                    Container::new(
                        Text::new(line)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
                );
            }
        }

        column = column.with_child(Label::new("MCP Servers").with_size(LabelSize::Sm).finish());

        let mcp_name_state = Rc::new(RefCell::new(String::new()));
        let mcp_source_state = Rc::new(RefCell::new(String::new()));
        let mcp_value_state = Rc::new(RefCell::new(String::new()));
        let mcp_name_input = TextInput::new()
            .with_placeholder("Server name")
            .with_on_change({
                let state = Rc::clone(&mcp_name_state);
                move |v| *state.borrow_mut() = v
            })
            .finish();
        let mcp_source_input = TextInput::new()
            .with_placeholder("Source (npm/github/local/url)")
            .with_on_change({
                let state = Rc::clone(&mcp_source_state);
                move |v| *state.borrow_mut() = v
            })
            .finish();
        let mcp_value_input = TextInput::new()
            .with_placeholder("Source value (package, repo, path or url)")
            .with_on_change({
                let state = Rc::clone(&mcp_value_state);
                move |v| *state.borrow_mut() = v
            })
            .finish();
        let state_for_mcp = Arc::clone(&state);
        let dirty_for_mcp = Rc::clone(&dirty);
        let mcp_install = Button::new(Text::new("Install MCP").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                let name = mcp_name_state.borrow().clone();
                let source = mcp_source_state.borrow().clone();
                let value = mcp_value_state.borrow().clone();
                if name.is_empty() || source.is_empty() {
                    log::warn!("mcp name and source are required");
                    return;
                }
                let id = uuid::Uuid::new_v4().to_string();
                let value = if value.is_empty() {
                    None
                } else {
                    Some(value.as_str())
                };
                if let Err(e) =
                    state_for_mcp.install_mcp_server(&id, &name, &source, value, vec![], None)
                {
                    log::error!("failed to install mcp server: {}", e);
                }
                *dirty_for_mcp.borrow_mut() = true;
            })
            .finish();
        column = column.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(mcp_name_input)
                    .with_child(mcp_source_input)
                    .with_child(mcp_value_input)
                    .with_child(mcp_install)
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );

        let mcps = state.list_mcp_servers().unwrap_or_default();
        if mcps.is_empty() {
            column = column.with_child(
                Text::new("No MCP servers installed yet.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            for mcp in mcps {
                let line = format!(
                    "{} | {} | {} | {}",
                    &mcp.id[..mcp.id.len().min(8)],
                    mcp.name,
                    mcp.source,
                    if mcp.auth_required {
                        "auth required"
                    } else {
                        "no auth"
                    }
                );
                column = column.with_child(
                    Container::new(
                        Text::new(line)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
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

impl Element for DriveViewPanel {
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
