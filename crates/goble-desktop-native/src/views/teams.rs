use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Avatar, Button, ButtonVariant, Checkbox, Container, CrossAxisAlignment,
    EdgeInsets, Element, EventContext, Fill, Flex, LayoutContext, PaintContext, Point, Scrollable,
    SizeConstraint, Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct TeamsViewPanel {
    content: Box<dyn Element>,
}

impl TeamsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        _ui_state: Rc<RefCell<crate::app::UiState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Self {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let agents = state.list_agents();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(
                Text::new("Teams")
                    .with_font_size(20.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );

        // Create team form.
        let team_id = Rc::new(RefCell::new(String::new()));
        let team_name = Rc::new(RefCell::new(String::new()));
        let team_metadata = Rc::new(RefCell::new(String::new()));
        let selected_agents: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let team_id_change = Rc::clone(&team_id);
        let team_name_change = Rc::clone(&team_name);
        let team_metadata_change = Rc::clone(&team_metadata);
        let state_for_create = Arc::clone(&state);
        let dirty_for_create = Rc::clone(&dirty);

        let mut create_form = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm)
            .with_child(
                Text::new("Create team")
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                TextInput::new()
                    .with_placeholder("Team ID")
                    .with_on_change(move |v| *team_id_change.borrow_mut() = v)
                    .finish(),
            )
            .with_child(
                TextInput::new()
                    .with_placeholder("Name")
                    .with_on_change(move |v| *team_name_change.borrow_mut() = v)
                    .finish(),
            )
            .with_child(
                TextInput::new()
                    .with_placeholder("Metadata (JSON)")
                    .with_on_change(move |v| *team_metadata_change.borrow_mut() = v)
                    .finish(),
            );

        for agent in &agents {
            let agent_id = agent.id.clone();
            let selected_agents_for_check = Rc::clone(&selected_agents);
            create_form = create_form.with_child(
                Checkbox::new()
                    .with_label(
                        Text::new(format!("{}", agent.name))
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_on_change(move |checked| {
                        let mut selected = selected_agents_for_check.borrow_mut();
                        if checked {
                            if !selected.contains(&agent_id) {
                                selected.push(agent_id.clone());
                            }
                        } else {
                            selected.retain(|id| id != &agent_id);
                        }
                    })
                    .finish(),
            );
        }

        create_form = create_form.with_child(
            Button::new(Text::new("Create").with_theme_color(ColorToken::Text, app).finish())
                .with_variant(ButtonVariant::Primary)
                .with_on_click(move || {
                    let id = team_id.borrow().clone();
                    let name = team_name.borrow().clone();
                    let metadata = team_metadata.borrow().clone();
                    let members = selected_agents.borrow().clone();
                    if id.is_empty() || name.is_empty() {
                        log::warn!("team id and name are required");
                        return;
                    }
                    if let Err(e) = state_for_create.create_team(&id, &name, &metadata, members) {
                        log::error!("failed to create team: {}", e);
                    }
                    *dirty_for_create.borrow_mut() = true;
                })
                .finish(),
        );

        column = column.with_child(
            Container::new(create_form.finish())
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
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm);
            for team in teams {
                let header = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        Avatar::new(&team.name)
                            .with_theme_background(ColorToken::Accent, app)
                            .with_theme_foreground(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(team.name.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .finish();

                let mut body = Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(header);
                if !team.metadata.is_empty() {
                    body = body.with_child(
                        Text::new(team.metadata.clone())
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                }
                if team.members.is_empty() {
                    body = body.with_child(
                        Text::new("No agents in this team.")
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                } else {
                    for member in &team.members {
                        body = body.with_child(
                            Text::new(format!("- {}", member))
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        );
                    }
                }

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

impl Element for TeamsViewPanel {
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
