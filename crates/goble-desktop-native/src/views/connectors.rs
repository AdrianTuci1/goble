use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Checkbox, ConnectorCard, Container, CrossAxisAlignment,
    EdgeInsets, Element, EventContext, Fill, Flex, Icon, LayoutContext, PaintContext, Point,
    Scrollable, SizeConstraint, Text, TextInput,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

pub struct ConnectorsViewPanel {
    content: Box<dyn Element>,
}

fn small_button(
    label: &str,
    on_click: impl FnMut() + 'static,
    app: &AppContext,
) -> Box<dyn Element> {
    Button::new(Text::new(label).with_theme_color(ColorToken::Text, app).finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(on_click)
        .finish()
}

impl ConnectorsViewPanel {
    pub fn new(
        state: Arc<DesktopState>,
        _ui_state: Rc<RefCell<crate::app::UiState>>,
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

        let search_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                TextInput::new()
                    .with_placeholder("Search MCP servers...")
                    .with_on_change(move |v| *query_for_change.borrow_mut() = v)
                    .finish(),
            )
            .with_child(
                Button::new(Text::new("Search").with_theme_color(ColorToken::Text, app).finish())
                    .with_variant(ButtonVariant::Primary)
                    .with_on_click(move || {
                        let query = query_for_search.borrow().clone();
                        let _results = state_for_search.search_mcp_servers(&query);
                        *dirty_for_search.borrow_mut() = true;
                    })
                    .finish(),
            )
            .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(
                Text::new("Connectors")
                    .with_font_size(20.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(search_row);

        // Manual install section.
        let install_id = Rc::new(RefCell::new(String::new()));
        let install_name = Rc::new(RefCell::new(String::new()));
        let install_source = Rc::new(RefCell::new("npm".to_string()));
        let install_value = Rc::new(RefCell::new(String::new()));
        let install_id_change = Rc::clone(&install_id);
        let install_name_change = Rc::clone(&install_name);
        let install_source_change = Rc::clone(&install_source);
        let install_value_change = Rc::clone(&install_value);
        let state_for_install = Arc::clone(&state);
        let dirty_for_install = Rc::clone(&dirty);

        column = column.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(
                        Text::new("Install MCP server")
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("ID")
                            .with_on_change(move |v| *install_id_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Name")
                            .with_on_change(move |v| *install_name_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Source (npm/github/local/url)")
                            .with_value("npm".to_string())
                            .with_on_change(move |v| *install_source_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        TextInput::new()
                            .with_placeholder("Package / repo / path / URL")
                            .with_on_change(move |v| *install_value_change.borrow_mut() = v)
                            .finish(),
                    )
                    .with_child(
                        Button::new(Text::new("Install").with_theme_color(ColorToken::Text, app).finish())
                            .with_variant(ButtonVariant::Primary)
                            .with_on_click(move || {
                                let id = install_id.borrow().clone();
                                let name = install_name.borrow().clone();
                                let source = install_source.borrow().clone();
                                let value = install_value.borrow().clone();
                                if id.is_empty() || name.is_empty() {
                                    log::warn!("id and name are required");
                                    return;
                                }
                                let source_value = if value.is_empty() { None } else { Some(value.as_str()) };
                                if let Err(e) = state_for_install.install_mcp_server(
                                    &id,
                                    &name,
                                    &source,
                                    source_value,
                                    vec![],
                                    None,
                                ) {
                                    log::error!("failed to install mcp server: {}", e);
                                }
                                *dirty_for_install.borrow_mut() = true;
                            })
                            .finish(),
                    )
                    .finish(),
            )
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );

        let servers = state.list_mcp_servers().unwrap_or_default();
        if servers.is_empty() {
            column = column.with_child(
                Text::new("No MCP servers installed.")
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        } else {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(sm);
            for server in servers {
                let server_id = server.id.clone();
                let name = server.name.clone();
                let description = format!(
                    "{} | capabilities: {}",
                    server.source,
                    server.capabilities.join(", ")
                );
                let tags: Vec<String> = server.capabilities.clone();

                let discover_id = server_id.clone();
                let state_for_discover = Arc::clone(&state);
                let dirty_for_discover = Rc::clone(&dirty);
                let discover = small_button("Discover", move || {
                    match state_for_discover.discover_mcp_tools(&discover_id) {
                        Ok(tools) => log::info!("discovered {} tools for {}", tools.len(), discover_id),
                        Err(e) => log::error!("failed to discover tools: {}", e),
                    }
                    *dirty_for_discover.borrow_mut() = true;
                }, app);

                let delete_id = server_id.clone();
                let state_for_delete = Arc::clone(&state);
                let dirty_for_delete = Rc::clone(&dirty);
                let delete = small_button("Delete", move || {
                    if let Err(e) = state_for_delete.delete_mcp_server(&delete_id) {
                        log::error!("failed to delete mcp server: {}", e);
                    }
                    *dirty_for_delete.borrow_mut() = true;
                }, app);

                let test_id = server_id.clone();
                let state_for_test = Arc::clone(&state);
                let dirty_for_test = Rc::clone(&dirty);
                let test = small_button("Test", move || {
                    let args = serde_json::json!({});
                    match state_for_test.test_call_mcp_tool(&test_id, "echo", args) {
                        Ok(res) => log::info!("test result: {}", res),
                        Err(e) => log::error!("test call failed: {}", e),
                    }
                    *dirty_for_test.borrow_mut() = true;
                }, app);

                let actions = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(discover)
                    .with_child(test)
                    .with_child(delete)
                    .finish();

                let card = ConnectorCard::new(
                    Icon::new("plug").finish(),
                    name,
                    description,
                    tags,
                    Some(actions),
                    app,
                )
                .finish();

                // Tool toggles.
                let mut tool_items: Vec<Box<dyn Element>> = Vec::new();
                for tool in &server.discovered_tools {
                    let tool = tool.clone();
                    let server_id = server_id.clone();
                    let enabled = server.enabled_tools.contains(&tool);
                    let state_for_toggle = Arc::clone(&state);
                    let dirty_for_toggle = Rc::clone(&dirty);
                    tool_items.push(
                        Checkbox::new()
                            .with_label(
                                Text::new(tool.clone())
                                    .with_theme_color(ColorToken::Text, app)
                                    .finish(),
                            )
                            .with_checked(enabled)
                            .with_on_change(move |checked| {
                                let mut enabled = state_for_toggle
                                    .list_mcp_servers()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .find(|s| s.id == server_id)
                                    .map(|s| s.enabled_tools)
                                    .unwrap_or_default();
                                if checked {
                                    if !enabled.contains(&tool) {
                                        enabled.push(tool.clone());
                                    }
                                } else {
                                    enabled.retain(|t| t != &tool);
                                }
                                if let Err(e) = state_for_toggle
                                    .update_mcp_server_meta(&server_id, vec![], enabled)
                                {
                                    log::error!("failed to update mcp meta: {}", e);
                                }
                                *dirty_for_toggle.borrow_mut() = true;
                            })
                            .finish(),
                    );
                }

                let tile = Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(card);
                let tile = if !tool_items.is_empty() {
                    tile.with_child(
                        Container::new(
                            Flex::column()
                                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .with_children(tool_items)
                                .finish(),
                        )
                        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                        .with_padding(EdgeInsets::uniform(sm))
                        .finish(),
                    )
                    .finish()
                } else {
                    tile.finish()
                };
                list = list.with_child(tile);
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

impl Element for ConnectorsViewPanel {
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
