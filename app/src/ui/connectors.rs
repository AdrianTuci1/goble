//! MCP connectors panel: installed server list + install/edit drawer.

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Checkbox, Chip, ConnectorCard, Container,
    CrossAxisAlignment, Divider, EdgeInsets, Element, Fill, Flex, Icon, Label, LabelSize,
    MainAxisSize, Rect, Scrollable, SearchInput, Select, SelectOption, Spacer, Switch, Text,
    TextInput, TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{AiActions, AiSnapshot, McpSearchEntry, McpServerEntry};

/// The install sources offered in the drawer, in select order.
const INSTALL_SOURCES: [&str; 4] = ["npm", "github", "local", "url"];

/// Connectors sheet content. When the install drawer is open it replaces the
/// list; otherwise the panel shows the installed servers.
pub fn build_connectors_sheet(
    app: &AppContext,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    if ai.install_open {
        build_install_drawer(app, ai, ai_actions)
    } else {
        build_connectors_list(app, ai, ai_actions)
    }
}

fn build_connectors_list(
    app: &AppContext,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_open_vault = ai_actions.on_open_vault.clone();
    let vault_button = TopbarButton::new(
        Icon::new("key")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_open_vault.borrow_mut())())
    .finish();

    let on_close = ai_actions.on_close_connectors.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("MCP Connectors")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(vault_button)
        .with_child(close_button)
        .finish();

    let on_search = ai_actions.on_connector_search_change.clone();
    let search = SearchInput::new()
        .with_value(ai.connector_search.clone())
        .with_placeholder("Search connectors…")
        .with_on_change(move |value| (on_search.borrow_mut())(value))
        .finish();

    let on_install_open = ai_actions.on_install_open.clone();
    let add_button = Button::new(Text::new("Add connector").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_install_open.borrow_mut())())
        .finish();

    let query = ai.connector_search.to_lowercase();
    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if ai.connectors.is_empty() {
        list = list.with_child(
            Text::new("No MCP servers installed yet. Add one to give agents more tools.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for server in ai.connectors.iter().filter(|s| {
        query.is_empty()
            || s.name.to_lowercase().contains(&query)
            || s.source.to_lowercase().contains(&query)
    }) {
        list = list.with_child(build_connector_row(app, server, ai_actions));
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(search);
    column = column.with_child(add_button);
    column = column.with_child(
        Label::new("Installed")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    column = column.with_child(Scrollable::new(list.finish(), Axis::Vertical).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// One installed MCP server row: status dot, name, source, tool counts,
/// enable switch, and discover/edit/delete actions.
fn build_connector_row(
    app: &AppContext,
    server: &McpServerEntry,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let radius = app.theme.radius_px();

    let status_color = if server.enabled_tools.is_empty() {
        ColorToken::Muted
    } else {
        ColorToken::Success
    };
    let status = if server.enabled_tools.is_empty() {
        "off"
    } else {
        "on"
    };

    let source_label = match &server.source_value {
        Some(v) if !v.is_empty() => format!("{} · {}", server.source, v),
        _ => server.source.clone(),
    };

    let name = Text::new(server.name.clone())
        .with_font_size(12.0)
        .with_theme_color(ColorToken::Text, app)
        .finish();
    let source = Text::new(source_label)
        .with_font_size(11.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish();
    let tools = Text::new(format!(
        "{} tools enabled · {} discovered · {}",
        server.enabled_tools.len(),
        server.discovered_tools.len(),
        status
    ))
    .with_font_size(11.0)
    .with_theme_color(ColorToken::Muted, app)
    .finish();

    let mut chips = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    for cap in server.capabilities.iter().take(4) {
        chips = chips
            .with_child(Chip::new(Text::new(cap.clone()).with_font_size(10.0).finish()).finish());
    }

    let on_toggle = ai_actions.on_connector_toggle.clone();
    let id_toggle = server.id.clone();
    let switch = Switch::new()
        .with_checked(!server.enabled_tools.is_empty())
        .with_on_change(move |enabled| (on_toggle.borrow_mut())(id_toggle.clone(), enabled))
        .finish();

    let on_discover = ai_actions.on_connector_discover.clone();
    let id_discover = server.id.clone();
    let discover_button = TopbarButton::new(
        Icon::new("refresh")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(26.0)
    .with_on_click(move || (on_discover.borrow_mut())(id_discover.clone()))
    .finish();

    let on_edit = ai_actions.on_install_edit.clone();
    let id_edit = server.id.clone();
    let edit_button = TopbarButton::new(
        Icon::new("sliders")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(26.0)
    .with_on_click(move || (on_edit.borrow_mut())(id_edit.clone()))
    .finish();

    let on_delete = ai_actions.on_connector_delete.clone();
    let id_delete = server.id.clone();
    let delete_button = TopbarButton::new(
        Icon::new("trash")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(26.0)
    .with_on_click(move || (on_delete.borrow_mut())(id_delete.clone()))
    .finish();

    let actions_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(discover_button)
        .with_child(edit_button)
        .with_child(delete_button)
        .finish();

    Container::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(6.0)
            .with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(spacing)
                    .with_child(
                        Container::new(Rect::new().with_size(vec2f(8.0, 8.0)).finish())
                            .with_background(Fill::Solid(app.theme.color(status_color)))
                            .with_corner_radius(4.0)
                            .finish(),
                    )
                    .with_child(name)
                    .with_child(Spacer::new().finish())
                    .with_child(switch)
                    .finish(),
            )
            .with_child(source)
            .with_child(tools)
            .with_child(chips.finish())
            .with_child(actions_row)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(radius)
    .with_padding(EdgeInsets::uniform(spacing))
    .finish()
}

/// Install/edit drawer: registry search results on top, manual form below
/// (name, source, source value, vault secrets).
fn build_install_drawer(
    app: &AppContext,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_back = ai_actions.on_install_close.clone();
    let back_button = TopbarButton::new(
        Icon::new("chevron-left")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_back.borrow_mut())())
    .finish();

    let on_close = ai_actions.on_close_connectors.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let title = if ai.install_editing_id.is_some() {
        "Edit connector"
    } else {
        "Install connector"
    };
    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(back_button)
        .with_child(
            Text::new(title)
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(close_button)
        .finish();

    // Registry search
    let on_search = ai_actions.on_install_search_change.clone();
    let search = SearchInput::new()
        .with_value(ai.install_search_query.clone())
        .with_placeholder("Search MCP registry…")
        .with_on_change(move |value| (on_search.borrow_mut())(value))
        .finish();

    let mut results = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if ai.install_search_results.is_empty() {
        results = results.with_child(
            Text::new("No registry results — configure the connector manually below.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for entry in &ai.install_search_results {
        results = results.with_child(build_search_result_card(app, entry, ai_actions));
    }

    // Manual form
    let on_name = ai_actions.on_install_name_change.clone();
    let name_input = TextInput::new()
        .with_value(ai.install_name.clone())
        .with_placeholder("Connector name")
        .with_on_change(move |value| (on_name.borrow_mut())(value))
        .finish();

    let sources: Vec<SelectOption> = INSTALL_SOURCES
        .iter()
        .map(|s| SelectOption::new(*s, *s))
        .collect();
    let selected_index = INSTALL_SOURCES
        .iter()
        .position(|s| s == &ai.install_source.as_str())
        .unwrap_or(0);
    let on_source = ai_actions.on_install_source_change.clone();
    let source_select = Select::new(sources)
        .with_selected_index(selected_index)
        .with_on_change(move |idx| {
            if let Some(i) = idx {
                if let Some(s) = INSTALL_SOURCES.get(i) {
                    (on_source.borrow_mut())(s.to_string());
                }
            }
        })
        .finish();

    let source_placeholder = match ai.install_source.as_str() {
        "npm" => "Package name, e.g. @modelcontextprotocol/server-filesystem",
        "github" => "owner/repo, optionally #rev",
        "local" => "Absolute path to the server directory",
        _ => "https://… server URL",
    };
    let on_source_value = ai_actions.on_install_source_value_change.clone();
    let source_value_input = TextInput::new()
        .with_value(ai.install_source_value.clone())
        .with_placeholder(source_placeholder)
        .with_on_change(move |value| (on_source_value.borrow_mut())(value))
        .finish();

    let secrets_section = build_secrets_section(app, ai, ai_actions);

    let mut form = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    form = form.with_child(name_input);
    form = form.with_child(source_select);
    form = form.with_child(source_value_input);
    form = form.with_child(secrets_section);
    if let Some(error) = &ai.install_error {
        form = form.with_child(
            Text::new(error.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        );
    }
    let form = form.finish();

    let on_submit = ai_actions.on_install_submit.clone();
    let submit_label = if ai.install_editing_id.is_some() {
        "Update"
    } else {
        "Install"
    };
    let submit_button = Button::new(Text::new(submit_label).finish())
        .with_variant(ButtonVariant::Primary)
        .with_disabled(ai.installing)
        .with_on_click(move || (on_submit.borrow_mut())())
        .finish();

    let on_cancel = ai_actions.on_install_close.clone();
    let cancel_button = Button::new(Text::new("Cancel").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_cancel.borrow_mut())())
        .finish();

    let buttons = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(cancel_button)
        .with_child(Spacer::new().finish())
        .with_child(submit_button)
        .finish();

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(
        Label::new("Find a server")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    column = column.with_child(search);
    column = column.with_child(Scrollable::new(results.finish(), Axis::Vertical).finish());
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(
        Label::new("Configure")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    column = column.with_child(form);
    column = column.with_child(buttons);

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Registry result card. Clicking the card prefills the manual form.
fn build_search_result_card(
    app: &AppContext,
    entry: &McpSearchEntry,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let auth = if entry.auth_required {
        "auth"
    } else {
        "no auth"
    };
    let mut tags = entry.capabilities.clone();
    tags.push(auth.to_string());

    let on_pick = ai_actions.on_install_pick.clone();
    let name = entry.name.clone();
    let source_kind = entry.source_kind.clone();
    ConnectorCard::new(
        Icon::new("plug")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        entry.name.clone(),
        entry.description.clone(),
        tags,
        None,
        app,
    )
    .with_on_click(move || {
        (on_pick.borrow_mut())(name.clone(), source_kind.clone(), String::new());
    })
    .finish()
}

/// Vault secrets selection inside the install drawer. When the vault is
/// locked it shows an inline unlock form instead.
fn build_secrets_section(
    app: &AppContext,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);

    let label = Label::new("Vault secrets")
        .with_size(LabelSize::Xs)
        .with_theme_color(ColorToken::Muted, app)
        .finish();

    if !ai.vault_unlocked {
        let on_draft = ai_actions.on_vault_unlock_draft_change.clone();
        let passphrase = TextInput::new()
            .with_value(ai.vault_unlock_draft.clone())
            .with_placeholder("Passphrase to unlock vault")
            .with_on_change(move |value| (on_draft.borrow_mut())(value))
            .finish();
        let on_unlock = ai_actions.on_vault_unlock.clone();
        let unlock_button = Button::new(Text::new("Unlock").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || (on_unlock.borrow_mut())())
            .finish();

        return Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing)
                .with_child(label)
                .with_child(
                    Text::new("Unlock the vault to attach secrets to this connector.")
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(passphrase)
                .with_child(unlock_button)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(spacing))
        .finish();
    }

    let mut secrets = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(4.0);
    if ai.vault_secrets.is_empty() {
        secrets = secrets.with_child(
            Text::new("No secrets yet — add some in the Vault panel.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for secret in &ai.vault_secrets {
        let selected = ai.install_selected_secrets.contains(&secret.key);
        let on_toggle = ai_actions.on_install_secret_toggle.clone();
        let key = secret.key.clone();
        let checkbox = Checkbox::new()
            .with_label(
                Text::new(secret.key.clone())
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_checked(selected)
            .with_on_change(move |checked| (on_toggle.borrow_mut())(key.clone(), checked))
            .finish();
        secrets = secrets.with_child(checkbox);
    }

    Container::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(label)
            .with_child(secrets.finish())
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(app.theme.radius_px())
    .with_padding(EdgeInsets::uniform(spacing))
    .finish()
}
