//! Drawing a page model: the settings elements in their places.
//!
//! One renderer for every page, so the row anatomy cannot drift page to page.
//! It walks [`PageModel`] in the same order [`PageModel::interactive_rows`]
//! enumerates, and the slot counter is what puts the focus ring on the row the
//! keyboard model says is focused.

use goble_core::config::ConfigFile;
use goble_ui::color::ColorU;
use goble_ui::elements::settings::{
    SettingsButton, SettingsNav, SettingsNavGroup, SettingsNavItem, SettingsNotice,
    SettingsNoticeKind, SettingsRow, SettingsRowStatus, SettingsRowStatusKind, SettingsSecretField,
    SettingsSection, SettingsSelect, SettingsSlider, SettingsTextField, SettingsToggle,
    SETTINGS_PAGE_MAX_WIDTH, SETTINGS_PAGE_PADDING, SETTINGS_RAIL_WIDTH, SETTINGS_SECTION_GAP,
};
use goble_ui::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, Divider, Element, Expanded, Fill,
    Flex, Icon, IconButton, MainAxisAlignment, MainAxisSize, Text,
};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::ColorToken;

use crate::ui::SettingsCategory;

use super::model::{ActionModel, ActionStyle, ControlModel, PageModel, RowModel, SectionModel};
use super::state::{SettingsAction, SettingsActions, SettingsUiState as State, StatusKind};

/// The rail: the grouped pages, filtered by the search query.
pub fn render_nav(
    app: &AppContext,
    settings: &State,
    actions: &SettingsActions,
) -> Box<dyn Element> {
    let query = settings.search.trim().to_string();
    let mut groups: Vec<SettingsNavGroup> = Vec::new();
    let mut visible_pages: Vec<SettingsCategory> = Vec::new();
    let mut selected = 0usize;
    let selected_page = settings.page;
    for (title, pages) in super::rail_groups() {
        let mut group = SettingsNavGroup::new(title);
        for page in pages {
            if query.is_empty() || super::page_matches(page, &query) {
                if page == selected_page {
                    selected = visible_pages.len();
                }
                visible_pages.push(page);
                group = group.with_item(SettingsNavItem::new(page.label()));
            }
        }
        if !group.items.is_empty() {
            groups.push(group);
        }
    }
    let rail_focused = if settings.region == super::SettingsRegion::Rail {
        Some(selected)
    } else {
        None
    };
    let pages = visible_pages.clone();
    let select_dispatch = actions.dispatch.clone();
    let search_dispatch = actions.dispatch.clone();
    let focus_dispatch = actions.dispatch.clone();
    let clear_dispatch = actions.dispatch.clone();
    let nav = SettingsNav::new(groups)
        .with_selected(selected)
        .with_focused(rail_focused)
        .with_search(settings.search.clone())
        .with_search_focused(settings.search_focused)
        .with_on_select(move |index| {
            if let Some(page) = pages.get(index).copied() {
                (select_dispatch.borrow_mut())(SettingsAction::Navigate(page));
            }
        })
        .with_on_search(move |query| {
            (search_dispatch.borrow_mut())(SettingsAction::Search(query));
        })
        .with_on_focus_change(move |focused| {
            (focus_dispatch.borrow_mut())(SettingsAction::FocusSearch(focused));
        })
        .with_on_clear(move || {
            (clear_dispatch.borrow_mut())(SettingsAction::ClearSearch);
        })
        .finish();
    ConstrainedBox::new(nav)
        .with_width(SETTINGS_RAIL_WIDTH)
        .finish()
}

/// The header band: the breadcrumb, the reload and open-file buttons.
pub fn render_header(
    app: &AppContext,
    settings: &State,
    actions: &SettingsActions,
    page: SettingsCategory,
) -> Box<dyn Element> {
    let crumb = Text::new("Settings")
        .with_font_size(13.0)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
    let slash = Text::new("/")
        .with_font_size(13.0)
        .with_color(app.theme.color(ColorToken::Border))
        .finish();
    let title = Text::new(page.label())
        .with_font_size(14.0)
        .with_weight(FontWeight::SemiBold)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(crumb)
        .with_child(slash)
        .with_child(title)
        .finish();
    let right = action_buttons(
        app,
        &[
            ActionModel::new("Reload", SettingsAction::Reload).with_icon("refresh"),
            ActionModel::new("Open file", SettingsAction::OpenFile(ConfigFile::Config))
                .with_icon("file"),
        ],
        actions,
    );
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(left)
        .with_child(right)
        .finish();
    let band = Container::new(row)
        .with_padding(goble_ui::elements::EdgeInsets::new(0.0, 20.0, 0.0, 12.0))
        .finish();
    let band = ConstrainedBox::new(band)
        .with_min_height(super::HEADER_HEIGHT)
        .with_height(super::HEADER_HEIGHT)
        .finish();
    let _ = settings;
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(band)
        .with_child(Divider::horizontal().finish())
        .finish()
}

/// The page: notice, status line and the sections.
pub fn render_page(
    app: &AppContext,
    model: &PageModel,
    settings: &State,
    actions: &SettingsActions,
) -> Box<dyn Element> {
    let mut children: Vec<Box<dyn Element>> = Vec::new();
    if let Some(notice) = settings.notice.as_ref() {
        children.push(render_notice(app, notice, actions));
    }
    let mut slot = 0usize;
    for section in &model.sections {
        children.push(render_section(app, section, settings, actions, &mut slot));
    }
    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(SETTINGS_SECTION_GAP)
        .with_children(children)
        .finish();
    let padded = Container::new(column)
        .with_padding(goble_ui::elements::EdgeInsets::uniform(
            SETTINGS_PAGE_PADDING,
        ))
        .finish();
    ConstrainedBox::new(padded)
        .with_max_width(SETTINGS_PAGE_MAX_WIDTH)
        .finish()
}

fn render_notice(
    app: &AppContext,
    notice: &super::state::SettingsNotice,
    actions: &SettingsActions,
) -> Box<dyn Element> {
    let kind = match notice.kind {
        StatusKind::Muted | StatusKind::Info => SettingsNoticeKind::Info,
        StatusKind::Success => SettingsNoticeKind::Success,
        StatusKind::Warning => SettingsNoticeKind::Warning,
        StatusKind::Error => SettingsNoticeKind::Error,
    };
    let mut element = SettingsNotice::new(kind, notice.text.clone());
    if let Some(action) = notice.action.as_ref() {
        let model = ActionModel::new(action.label.clone(), action.action.clone()).primary();
        element = element.with_action(action_button(app, &model, actions));
    }
    element.finish()
}

fn render_section(
    app: &AppContext,
    section: &SectionModel,
    settings: &State,
    actions: &SettingsActions,
    slot: &mut usize,
) -> Box<dyn Element> {
    let mut element = SettingsSection::new(section.title.clone());
    if !section.description.is_empty() {
        element = element.with_description(section.description.clone());
    }
    if let Some(action) = section.header_action.as_ref() {
        element = element.with_header_action(action_button(app, action, actions));
    }
    if section.plain {
        element = element.plain();
    }
    for row in &section.rows {
        element = element.with_row(render_row(app, row, settings, actions, slot));
    }
    element.finish()
}

fn render_row(
    app: &AppContext,
    row: &RowModel,
    settings: &State,
    actions: &SettingsActions,
    slot: &mut usize,
) -> Box<dyn Element> {
    let interactive = !row.disabled && row.control.is_interactive();
    // The page's focus ring only shows while the page owns the keyboard; when
    // the rail does, the rail's row is the one ringed.
    let focused = settings.region == super::SettingsRegion::Page
        && interactive
        && *slot == settings.row_index;
    if interactive {
        *slot += 1;
    }
    // A field holding the caret shows its own ring; the row keeps the focus
    // ring only when no field in it owns the caret (only one 2px ring is
    // visible on a row, as the parity document requires).
    let row_ring = focused && settings.focused_field.is_none();
    let mut element =
        SettingsRow::new(app, row.label.clone(), row.hint.clone()).with_focused(row_ring);
    if row.disabled {
        element = element.with_disabled(true);
    }
    if let Some((text, kind)) = row.status.as_ref() {
        element = element.with_status(SettingsRowStatus::new(text.clone(), row_status_kind(*kind)));
    }
    if let Some(reset) = row.reset.as_ref() {
        element = element.with_reset(reset_button(app, reset, actions));
    }
    if let Some(control) = render_control(app, row, settings, actions, focused) {
        element = element.with_control(control);
    }
    if !row.children.is_empty() {
        let mut child_slot = *slot;
        let mut children: Vec<Box<dyn Element>> = Vec::new();
        for child in &row.children {
            children.push(render_row(app, child, settings, actions, &mut child_slot));
        }
        let body = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(4.0)
            .with_children(children)
            .finish();
        // The children were counted after their parent, matching
        // `PageModel::interactive_rows`'s flatten order.
        *slot = child_slot;
        element = element.with_body(body);
    }
    element.finish()
}

fn row_status_kind(kind: StatusKind) -> SettingsRowStatusKind {
    match kind {
        StatusKind::Muted => SettingsRowStatusKind::Muted,
        StatusKind::Info => SettingsRowStatusKind::Info,
        StatusKind::Success => SettingsRowStatusKind::Success,
        StatusKind::Warning => SettingsRowStatusKind::Unavailable,
        StatusKind::Error => SettingsRowStatusKind::Error,
    }
}

fn render_control(
    app: &AppContext,
    row: &RowModel,
    settings: &State,
    actions: &SettingsActions,
    focused: bool,
) -> Option<Box<dyn Element>> {
    let dispatch = actions.dispatch.clone();
    let control = match &row.control {
        // each arm moves its own clone of the dispatch into the control's callback
        ControlModel::Toggle {
            path,
            value,
            disabled,
        } => {
            let path = path.clone();
            let next = !value;
            SettingsToggle::new(*value)
                .with_disabled(*disabled)
                .with_on_change(move || {
                    (dispatch.clone().borrow_mut())(SettingsAction::SetToggle {
                        path: path.clone(),
                        value: next,
                    });
                })
                .finish()
        }
        ControlModel::Select {
            path,
            options,
            selected,
            disabled,
        } => {
            let path = path.clone();
            SettingsSelect::new(options.clone())
                .with_selected(*selected)
                .with_disabled(*disabled)
                .with_focused(focused)
                .with_on_change(move |index| {
                    (dispatch.clone().borrow_mut())(SettingsAction::SetSelect {
                        path: path.clone(),
                        index,
                    });
                })
                .finish()
        }
        ControlModel::Text {
            id,
            value,
            placeholder,
            secret,
            configured,
            disabled,
        } => {
            let focused_field = settings.focused_field.as_deref() == Some(id.as_str());
            let id_change = id.clone();
            let id_commit = id.clone();
            let change_dispatch = dispatch.clone();
            let submit_dispatch = dispatch.clone();
            let on_change = move |value: String| {
                (change_dispatch.borrow_mut())(SettingsAction::SetDraft {
                    id: id_change.clone(),
                    value,
                });
            };
            let on_submit = move || {
                (submit_dispatch.borrow_mut())(SettingsAction::CommitField {
                    id: id_commit.clone(),
                });
            };
            if *secret {
                SettingsSecretField::new()
                    .with_configured(*configured)
                    .with_draft(value.clone())
                    .with_focused(focused_field)
                    .with_disabled(*disabled)
                    .with_width(240.0)
                    .with_on_change(on_change)
                    .with_on_submit(on_submit)
                    .finish()
            } else {
                SettingsTextField::new()
                    .with_value(value.clone())
                    .with_placeholder(placeholder.clone())
                    .with_focused(focused_field)
                    .with_disabled(*disabled)
                    .with_width(240.0)
                    .with_on_change(on_change)
                    .with_on_submit(on_submit)
                    .finish()
            }
        }
        ControlModel::Slider {
            path,
            value,
            min,
            max,
            step: _,
        } => {
            let path = path.clone();
            let slider = SettingsSlider::new(*value, *min, *max)
                .with_width(160.0)
                .with_on_change(move |value| {
                    (dispatch.clone().borrow_mut())(SettingsAction::SetSlider {
                        path: path.clone(),
                        value,
                    });
                })
                .finish();
            let output = super::pages::format_number(*value);
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0)
                .with_child(slider_output(app, output))
                .with_child(slider)
                .finish()
        }
        ControlModel::ReadOnly { text } => {
            let mut parts: Vec<Box<dyn Element>> = Vec::new();
            if let Some(colour) = ColorU::from_hex(text) {
                let swatch = Container::new(goble_ui::elements::Empty::new().finish())
                    .with_background(Fill::Solid(colour))
                    .with_border(
                        goble_ui::elements::Border::all(1.0)
                            .with_border_color(app.theme.color(ColorToken::Border)),
                    )
                    .with_corner_radius(4.0)
                    .finish();
                parts.push(
                    ConstrainedBox::new(swatch)
                        .with_width(14.0)
                        .with_height(14.0)
                        .finish(),
                );
            }
            parts.push(
                Text::new(text.clone())
                    .with_font_size(12.0)
                    .with_color(app.theme.color(ColorToken::Text))
                    .finish(),
            );
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.0)
                .with_children(parts)
                .finish()
        }
        ControlModel::Buttons(button_models) => action_buttons(app, button_models, actions),
        ControlModel::None => return None,
    };
    Some(control)
}

/// The `<output>` box beside a slider.
fn slider_output(app: &AppContext, text: String) -> Box<dyn Element> {
    let label = Text::new(text)
        .with_font_size(11.0)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let label = ConstrainedBox::new(label).with_min_width(28.0).finish();
    Container::new(
        Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_child(label)
            .finish(),
    )
    .with_padding(goble_ui::elements::EdgeInsets::new(4.0, 8.0, 4.0, 8.0))
    .with_background(Fill::Solid(
        app.theme
            .color(ColorToken::Bg)
            .mix(&app.theme.color(ColorToken::SurfaceRaised), 0.7),
    ))
    .with_corner_radius(6.0)
    .finish()
}

fn action_buttons(
    app: &AppContext,
    actions: &[ActionModel],
    settings: &SettingsActions,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0);
    for action in actions {
        row = row.with_child(action_button(app, action, settings));
    }
    row.finish()
}

fn action_button(
    app: &AppContext,
    action: &ActionModel,
    settings: &SettingsActions,
) -> Box<dyn Element> {
    let _ = app;
    let dispatch = settings.dispatch.clone();
    let forwarded = action.action.clone();
    let mut button = SettingsButton::new(action.label.clone()).with_disabled(action.disabled);
    if let Some(icon) = action.icon {
        button = button.with_icon(icon);
    }
    match action.style {
        ActionStyle::Primary => button = button.primary(),
        ActionStyle::Destructive => button = button.destructive(),
        ActionStyle::Ghost => button = button.ghost(),
        ActionStyle::Default => {}
    }
    button
        .with_on_click(move || {
            (dispatch.borrow_mut())(forwarded.clone());
        })
        .finish()
}

fn reset_button(
    app: &AppContext,
    action: &SettingsAction,
    settings: &SettingsActions,
) -> Box<dyn Element> {
    let dispatch = settings.dispatch.clone();
    let forwarded = action.clone();
    IconButton::new(
        Icon::new("undo")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(vec2f(20.0, 20.0))
    .with_on_click(move || {
        (dispatch.borrow_mut())(forwarded.clone());
    })
    .finish()
}
