//! Drawing a page model: the settings elements in their places.
//!
//! One renderer for every page, so the row anatomy cannot drift page to page.
//! It walks [`PageModel`] in the same order [`PageModel::interactive_rows`]
//! enumerates, and the slot counter is what puts the focus ring on the row the
//! keyboard model says is focused.

#[path = "color_palette.rs"]
pub mod color_palette;

use std::cell::RefCell;
use std::rc::Rc;

use goble_core::config::ConfigFile;
use goble_ui::color::ColorU;
use goble_ui::elements::settings::{
    SettingsButton, SettingsKeyChip, SettingsNav, SettingsNavGroup, SettingsNavItem,
    SettingsNotice, SettingsNoticeKind, SettingsRow, SettingsRowStatus, SettingsRowStatusKind,
    SettingsSecretField, SettingsSection, SettingsSegmented, SettingsSelect, SettingsSlider,
    SettingsTextField, SettingsToggle, SETTINGS_PAGE_MAX_WIDTH, SETTINGS_PAGE_PADDING,
    SETTINGS_RAIL_WIDTH, SETTINGS_SECTION_GAP, SETTINGS_SHEET_PADDING, SETTINGS_SHEET_WIDTH,
};
use goble_ui::elements::{
    AppContext, Axis, ConstrainedBox, Container, CrossAxisAlignment, Divider, Element,
    EventContext, Expanded, Fill, Flex, Icon, IconButton, LayoutContext, MainAxisAlignment,
    MainAxisSize, PaintContext, Point, Scrollable, SearchInput, SizeConstraint, Text, Tooltip,
    TooltipPosition,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::ColorToken;

use crate::ui::SettingsCategory;

use super::model::{
    ActionModel, ActionStyle, ControlModel, PageModel, PageSearch, RowModel, SectionModel,
};
use super::state::{SettingsAction, SettingsActions, SettingsUiState as State, StatusKind};

/// The rail: the grouped pages, filtered by the search query, with the compact
/// account footer at its bottom.
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
    let nav = SettingsNav::new(groups)
        .with_selected(selected)
        .with_focused(rail_focused)
        .without_search()
        .with_on_select(move |index| {
            if let Some(page) = pages.get(index).copied() {
                (select_dispatch.borrow_mut())(SettingsAction::Navigate(page));
            }
        })
        .finish();
    let nav = ConstrainedBox::new(nav)
        .with_width(SETTINGS_RAIL_WIDTH)
        .finish();
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(nav).finish())
        .finish()
}

/// The compact account footer every settings rail ends with: the avatar, the
/// user's friendly name (or "You" until one is set) and the local workspace
/// line. Shared by the in-pane rail and the floating overlay rail so the two
/// cannot drift.
pub fn render_rail_footer(app: &AppContext, settings: &State) -> Box<dyn Element> {
    let name = super::fields::app_text(settings, "friendly_name");
    let who = if name.trim().is_empty() {
        "You".to_string()
    } else {
        name
    };
    let initial = who.chars().next().unwrap_or('?').to_uppercase().to_string();
    let avatar = Container::new(
        Text::new(initial)
            .with_font_size(12.0)
            .with_weight(FontWeight::SemiBold)
            .with_color(app.theme.color(ColorToken::Text))
            .finish(),
    )
    .with_padding(goble_ui::elements::EdgeInsets::uniform(10.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Hover)))
    .with_corner_radius(16.0)
    .finish();
    let labels = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(1.0)
        .with_child(
            Text::new(who)
                .with_font_size(13.0)
                .with_weight(FontWeight::Medium)
                .with_color(app.theme.color(ColorToken::Text))
                .finish(),
        )
        .with_child(
            Text::new("Local workspace")
                .with_font_size(11.0)
                .with_color(app.theme.color(ColorToken::Muted))
                .finish(),
        )
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(avatar)
        .with_child(labels)
        .finish();
    Container::new(row)
        .with_padding(goble_ui::elements::EdgeInsets::new(8.0, 6.0, 8.0, 6.0))
        .finish()
}

/// The header band: the breadcrumb, the reload and open-file buttons, and the
/// X that closes the settings pane at the trailing end.
///
/// The band is the compact DSH/T3 settings header (`h-9`, 36pt): a 28pt button
/// row fits inside it with no vertical padding, so the page title keeps the top
/// of the surface while the page gains 12pt of height against the old 48pt bar.
pub fn render_header(
    app: &AppContext,
    settings: &State,
    actions: &SettingsActions,
    page: SettingsCategory,
    close: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let crumb = Text::new("Settings")
        .with_font_size(12.0)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
    let slash = Text::new("/")
        .with_font_size(12.0)
        .with_color(app.theme.color(ColorToken::Border))
        .finish();
    let title = Text::new(page.label())
        .with_font_size(13.0)
        .with_weight(FontWeight::SemiBold)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(7.0)
        .with_child(crumb)
        .with_child(slash)
        .with_child(title)
        .finish();
    let reload = action_button(
        app,
        &ActionModel::new("Reload", SettingsAction::Reload).with_icon("refresh"),
        actions,
    );
    let open = action_button(
        app,
        &ActionModel::new("Open file", SettingsAction::OpenFile(ConfigFile::Config))
            .with_icon("file"),
        actions,
    );
    let close_button = IconButton::new(
        Icon::new("x")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(vec2f(24.0, 24.0))
    .with_on_click(move || (close.borrow_mut())())
    .finish();
    let close_button = Tooltip::new(close_button, "Close settings")
        .with_position(TooltipPosition::Below)
        .finish();
    let right = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(reload)
        .with_child(open)
        .with_child(close_button)
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(left)
        .with_child(right)
        .finish();
    // EdgeInsets::new(left, top, right, bottom): horizontal inset only, so the
    // 31.6pt button row is the band's height and the compact header really is
    // shorter than the old 48pt band (which had 20pt of top padding).
    let band = Container::new(row)
        .with_padding(goble_ui::elements::EdgeInsets::new(16.0, 0.0, 10.0, 0.0))
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
    let content = render_page_content(app, model, settings, actions);
    // The Appearance menu is painted after every section but receives pointer
    // and key events before them: a panel anchored under an early row must draw
    // over the rows below it, and a press inside it must not reach a control
    // that happens to sit underneath.
    let overlay = color_palette::appearance_menu_layer(app, settings, actions);
    Box::new(color_palette::OverlayLayer::new(content, overlay))
}

/// The page's sections/search without the Appearance popover layer. The
/// Settings overlay mounts that layer above its own Scrollable so the panel is
/// not clipped by the scroll viewport.
pub fn render_page_content(
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
    let section_count = model.sections.len();
    for (index, section) in model.sections.iter().enumerate() {
        children.push(render_section(app, section, settings, actions, &mut slot));
        if index + 1 < section_count {
            // A run of folds (the per-CLI groups) is a tight list: 10pt between
            // a heading and the next, while ordinary page sections keep T3's
            // 32pt. The gap is a real element so the flex needs no spacing.
            let next_is_fold = model
                .sections
                .get(index + 1)
                .is_some_and(|next| next.fold.is_some());
            let gap = if section.fold.is_some() || next_is_fold {
                10.0
            } else {
                SETTINGS_SECTION_GAP
            };
            children.push(
                goble_ui::elements::Empty::new()
                    .with_size(vec2f(0.0, gap))
                    .finish(),
            );
        }
    }
    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
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

/// The page-level search field: the same `SearchInput` the rail uses, sized to
/// the page and given the reference's rounded box and magnifier. Its value and
/// focus live in the app state, so a frame rebuild neither drops the query nor
/// pulls the caret out.
fn render_page_search(
    app: &AppContext,
    search: &PageSearch,
    actions: &SettingsActions,
) -> Box<dyn Element> {
    let change_dispatch = actions.dispatch.clone();
    let focus_dispatch = actions.dispatch.clone();
    let kind = search.kind;
    let field = SearchInput::new()
        .with_value(search.value.clone())
        .with_placeholder(search.placeholder.clone())
        .with_focused(search.focused)
        .with_compact(true)
        .with_icon(true)
        .with_extra_height(6.0)
        .with_text_size(13.0)
        .with_corner_radius(8.0)
        .with_on_change(move |value| {
            (change_dispatch.borrow_mut())(kind.change_action(value));
        })
        .with_on_focus_change(move |focused| {
            (focus_dispatch.borrow_mut())(kind.focus_action(focused));
        })
        .finish();
    let _ = app;
    Container::new(field)
        .with_padding(goble_ui::elements::EdgeInsets::new(0.0, 0.0, 0.0, 16.0))
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
    // K: Settings carry no description copy. Section headings, labels, values
    // and controls are the whole page; the model keeps the strings for other
    // surfaces (search indexing) but the sheet does not draw them.
    if let Some(action) = section.header_action.as_ref() {
        element = element.with_header_action(action_button(app, action, actions));
    }
    if section.plain {
        element = element.plain();
    }
    if let Some(fold) = section.fold.as_ref() {
        let dispatch = actions.dispatch.clone();
        let forwarded = fold.action.clone();
        element = element
            .with_fold(fold.open)
            .compact_header()
            .with_on_fold_toggle(move || {
                (dispatch.borrow_mut())(forwarded.clone());
            });
    }
    // A closed fold draws its heading only; its rows are left out of the same
    // order the keyboard model counts.
    if section.is_open() {
        for row in &section.rows {
            element = element.with_row(render_row(app, row, settings, actions, slot));
        }
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
        SettingsRow::new(app, row.label.clone(), String::new()).with_focused(row_ring);
    if let Some(swatch) = row.swatch {
        element = element.with_swatch(swatch);
    }
    if row.disabled {
        element = element.with_disabled(true);
    }
    // K: muted status lines were descriptive subtitles ("Using the palette's
    // accent"); keep only real feedback (info/success/warning/error).
    if let Some((text, kind)) = row.status.as_ref() {
        if !matches!(kind, super::state::StatusKind::Muted) {
            element =
                element.with_status(SettingsRowStatus::new(text.clone(), row_status_kind(*kind)));
        }
    }
    if let Some(reset) = row.reset.as_ref() {
        element = element.with_reset(reset_button(app, reset, actions));
    }
    if let Some(control) = render_control(app, row, settings, actions, focused, false) {
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
    full_width: bool,
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
            // The Appearance page's theme choice is the one select whose real
            // dropdown lives at page level: its panel has to paint after the
            // rows it covers. Every other select keeps the shared control.
            if path == "theme.preset" {
                let swatch = options.get(*selected).and_then(|label| {
                    goble_ui::theme::ThemePreset::ALL
                        .into_iter()
                        .find(|preset| preset.display_name() == label)
                        .map(|preset| preset.accent())
                });
                return Some(color_palette::theme_select_control(
                    settings, actions, options, *selected, swatch, *disabled, focused,
                ));
            }
            let path = path.clone();
            // The open flag is app-owned, keyed by the control path: the tree is
            // rebuilt every frame, so an element-local flag would shut the menu
            // on the frame after it opened. Per-select menu ids keep the window's
            // one-menu rule and the outside-press dismiss working.
            let (open, menu_id) = settings.select_menu(&path);
            let open_changed = Rc::clone(&settings.select_open);
            let own_path = path.clone();
            SettingsSelect::new(options.clone())
                .with_selected(*selected)
                .with_disabled(*disabled)
                .with_focused(focused)
                .with_open(open)
                .with_menu_id(menu_id)
                .with_on_open_change(move |is_open| {
                    if !is_open {
                        return;
                    }
                    // Only one plain select menu may be up even if a second one
                    // was opened from the keyboard before the registry saw it.
                    for (candidate, cell) in open_changed.borrow().iter() {
                        if candidate != &own_path {
                            *cell.borrow_mut() = false;
                        }
                    }
                })
                .with_on_change(move |index| {
                    (dispatch.clone().borrow_mut())(SettingsAction::SetSelect {
                        path: path.clone(),
                        index,
                    });
                })
                .finish()
        }
        ControlModel::Segmented {
            path,
            options,
            selected,
            disabled,
        } => {
            let path = path.clone();
            let dispatch = dispatch.clone();
            SettingsSegmented::new(options.clone())
                .with_selected(*selected)
                .with_disabled(*disabled)
                .with_focused(focused)
                .with_on_change(move |index| {
                    (dispatch.borrow_mut())(super::model::segment_action(&path, index));
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
            if !*secret {
                if let Some((channel, key)) = color_palette::channel_for_path(id) {
                    let color = color_palette::channel_color(settings, key);
                    return Some(color_palette::color_palette_control(
                        settings,
                        actions,
                        id,
                        channel,
                        value.clone(),
                        color,
                        *disabled,
                        focused,
                    ));
                }
            }
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
                let field = SettingsSecretField::new()
                    .with_configured(*configured)
                    .with_draft(value.clone())
                    .with_focused(focused_field)
                    .with_disabled(*disabled);
                let field = if full_width {
                    field
                } else {
                    field.with_width(240.0)
                };
                field
                    .with_on_change(on_change)
                    .with_on_submit(on_submit)
                    .finish()
            } else {
                let field = SettingsTextField::new()
                    .with_value(value.clone())
                    .with_placeholder(placeholder.clone())
                    .with_focused(focused_field)
                    .with_disabled(*disabled);
                let field = if full_width {
                    field
                } else {
                    field.with_width(240.0)
                };
                field
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
        ControlModel::KeyChips { keys, action } => {
            let chip = SettingsKeyChip::new(keys.clone()).finish();
            let mut parts: Vec<Box<dyn Element>> = vec![chip];
            if let Some(action) = action {
                parts.push(icon_action_button(app, action, actions));
            }
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0)
                .with_children(parts)
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

/// The add/edit side sheet: a docked panel on the settings pane's trailing
/// edge, with its own header (title, close X), the form's rows and the same
/// scroll state the page keeps for its own list.
pub fn render_side_sheet(
    app: &AppContext,
    section: &SectionModel,
    settings: &State,
    actions: &SettingsActions,
) -> Box<dyn Element> {
    let dispatch = actions.dispatch.clone();
    let title = Text::new(section.title.clone())
        .with_font_size(14.0)
        .with_weight(FontWeight::SemiBold)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let heading = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(title)
        .with_child(
            IconButton::new(
                Icon::new("x")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(vec2f(24.0, 24.0))
            .with_on_click(move || (dispatch.borrow_mut())(SettingsAction::CancelForm))
            .finish(),
        )
        .finish();
    let mut children: Vec<Box<dyn Element>> = vec![heading];
    if !section.description.is_empty() {
        children.push(
            Container::new(
                Text::new(section.description.clone())
                    .with_font_size(12.0)
                    .with_line_height(1.45)
                    .with_color(app.theme.color(ColorToken::Muted))
                    .finish(),
            )
            .with_padding(goble_ui::elements::EdgeInsets::new(6.0, 0.0, 0.0, 0.0))
            .finish(),
        );
    }
    if let Some((kind, text)) = settings.status.as_ref() {
        if matches!(kind, StatusKind::Error) {
            children.push(
                Container::new(
                    Text::new(text.clone())
                        .with_font_size(11.0)
                        .with_color(app.theme.color(ColorToken::Error))
                        .finish(),
                )
                .with_padding(goble_ui::elements::EdgeInsets::new(8.0, 0.0, 0.0, 0.0))
                .finish(),
            );
        }
    }
    let header = Container::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_children(children)
            .finish(),
    )
    .with_padding(goble_ui::elements::EdgeInsets::new(
        SETTINGS_SHEET_PADDING,
        SETTINGS_SHEET_PADDING,
        13.0,
        SETTINGS_SHEET_PADDING,
    ))
    .finish();
    let divider = Divider::horizontal().finish();
    let mut slot = 0usize;
    let mut form_children: Vec<Box<dyn Element>> = Vec::new();
    for row in &section.rows {
        form_children.push(render_sheet_row(app, row, settings, actions, &mut slot));
    }
    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_children(form_children)
        .finish();
    let body = Container::new(body)
        .with_padding(goble_ui::elements::EdgeInsets::new(
            0.0,
            SETTINGS_SHEET_PADDING,
            16.0,
            SETTINGS_SHEET_PADDING,
        ))
        .finish();
    let scroll = Scrollable::new(body, Axis::Vertical)
        .with_state(Rc::clone(&settings.sheet_scroll))
        .finish();
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(divider)
        .with_child(Expanded::new(scroll).finish())
        .finish();
    ConstrainedBox::new(
        Container::new(column)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(
                goble_ui::elements::Border::all(1.0)
                    .with_border_color(app.theme.color(ColorToken::Border)),
            )
            .finish(),
    )
    .with_width(SETTINGS_SHEET_WIDTH)
    .finish()
}

/// One sheet row: the label, its description, and the control full-width under
/// them (a side panel is too narrow for T3's label/control columns).
fn render_sheet_row(
    app: &AppContext,
    row: &RowModel,
    settings: &State,
    actions: &SettingsActions,
    slot: &mut usize,
) -> Box<dyn Element> {
    let interactive = !row.disabled && row.control.is_interactive();
    let focused = settings.region == super::SettingsRegion::Sheet
        && interactive
        && *slot == settings.sheet_row;
    if interactive {
        *slot += 1;
    }
    let row_ring = focused && settings.focused_field.is_none();
    let mut element =
        SettingsRow::new(app, row.label.clone(), String::new()).with_focused(row_ring);
    if let Some(swatch) = row.swatch {
        element = element.with_swatch(swatch);
    }
    if row.disabled {
        element = element.with_disabled(true);
    }
    // K: muted status lines were descriptive subtitles ("Using the palette's
    // accent"); keep only real feedback (info/success/warning/error).
    if let Some((text, kind)) = row.status.as_ref() {
        if !matches!(kind, super::state::StatusKind::Muted) {
            element =
                element.with_status(SettingsRowStatus::new(text.clone(), row_status_kind(*kind)));
        }
    }
    if let ControlModel::ReadOnly { text } = &row.control {
        element = element.with_body(
            Text::new(text.clone())
                .with_font_size(13.0)
                .with_color(app.theme.color(ColorToken::Text))
                .finish(),
        );
    } else if let Some(control) = render_control(app, row, settings, actions, focused, true) {
        element = element.with_body(control);
    }
    element.finish()
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

/// The trailing icon action a key-chip row carries (the Shortcuts page's
/// trash). It is a real `SettingsButton`, so the click goes through the same
/// dispatch as every other settings control; the tooltip states the honest
/// outcome before the press.
fn icon_action_button(
    app: &AppContext,
    action: &ActionModel,
    settings: &SettingsActions,
) -> Box<dyn Element> {
    let dispatch = settings.dispatch.clone();
    let forwarded = action.action.clone();
    let button = SettingsButton::icon(action.icon.unwrap_or("trash"))
        .with_disabled(action.disabled)
        .ghost()
        .with_on_click(move || {
            (dispatch.borrow_mut())(forwarded.clone());
        })
        .finish();
    let _ = app;
    Tooltip::new(button, action.label.clone())
        .with_position(TooltipPosition::Above)
        .finish()
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

/// The sheet's outside-click/dim layer: paints the page dimmed and consumes the
/// first pointer press as "dismiss the sheet", so the main surface behind a
/// sheet is a focus trap rather than a second interactive region.
pub struct DismissLayer {
    child: Box<dyn Element>,
    tint: ColorU,
    on_dismiss: Rc<RefCell<dyn FnMut()>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl DismissLayer {
    pub fn new(
        child: Box<dyn Element>,
        tint: ColorU,
        on_dismiss: Rc<RefCell<dyn FnMut()>>,
    ) -> Self {
        Self {
            child,
            tint,
            on_dismiss,
            size: None,
            origin: None,
        }
    }
}

impl Element for DismissLayer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
        if let (Some(size), Some(renderer)) = (self.size, ctx.renderer.as_mut()) {
            renderer.fill_rect(rectf(origin.x, origin.y, size.x, size.y), self.tint);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        match event {
            DispatchedEvent::MouseDown { .. } => {
                (self.on_dismiss.borrow_mut())();
                true
            }
            // Nothing behind the sheet receives hover or drag while it is open.
            DispatchedEvent::MouseMove { .. }
            | DispatchedEvent::MouseUp { .. }
            | DispatchedEvent::Scroll { .. } => true,
            _ => false,
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use goble_ui::test_util::render_element;

    fn press(page: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) -> bool {
        let mut ctx = EventContext::default();
        let down = page.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            &mut ctx,
            app,
        );
        let up = page.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
            &mut ctx,
            app,
        );
        down && up
    }

    fn text_origin(
        commands: &[goble_ui::render::RenderCommand],
        needle: &str,
    ) -> Option<Vector2F> {
        commands.iter().find_map(|command| match command {
            goble_ui::render::RenderCommand::DrawText { origin, text, .. } if text == needle => {
                Some(*origin)
            }
            _ => None,
        })
    }

    #[test]
    fn a_plain_select_stays_open_across_the_next_page_build() {
        let state = State::default();
        let actions = SettingsActions::new(|_| {});
        let app = AppContext::default();
        let model = super::super::pages::page_model(SettingsCategory::General, &state);

        // The page has to be laid out and painted before an event can hit-test
        // it, exactly as the root view does each frame.
        let mut page = render_page(&app, &model, &state, &actions);
        let commands = render_element(&mut page, vec2f(900.0, 2000.0), &app);
        let origin = text_origin(&commands, "Default").expect("the Permissions trigger label");
        assert!(press(
            &mut page,
            &app,
            vec2f(origin.x + 2.0, origin.y + 2.0)
        ));
        let (open, _) = state.select_menu("app.approval_mode");
        assert!(*open.borrow(), "the press opens the Permissions select");

        // The next frame rebuilds the whole page; the app-owned cell keeps the
        // menu up and the renderer draws its options again.
        let model = super::super::pages::page_model(SettingsCategory::General, &state);
        let mut page = render_page(&app, &model, &state, &actions);
        let commands = render_element(&mut page, vec2f(900.0, 2000.0), &app);
        assert!(
            text_origin(&commands, "Plan").is_some(),
            "the open select still draws its menu"
        );

        // Choosing a row closes it.
        let origin = text_origin(&commands, "Plan").expect("the menu row");
        assert!(press(
            &mut page,
            &app,
            vec2f(origin.x + 2.0, origin.y + 2.0)
        ));
        assert!(!*open.borrow(), "choosing a row closes the select");
    }

    fn texts(commands: &[goble_ui::render::RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                goble_ui::render::RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_shortcuts_trash_is_a_real_button_that_dispatches_the_honest_remap() {
        let app = AppContext::default();
        let state = State::default();
        let dispatched: Rc<RefCell<Vec<SettingsAction>>> = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&dispatched);
        let actions = SettingsActions::new(move |action| seen.borrow_mut().push(action));
        let model = super::super::pages::page_model(SettingsCategory::Shortcuts, &state);
        let mut page = render_page(&app, &model, &state, &actions);
        let commands = render_element(&mut page, vec2f(900.0, 2400.0), &app);

        // The chips draw the grouped chord before the trash button.
        assert!(texts(&commands).iter().any(|text| text == "⌘ + K"));

        let (origin, size) = commands
            .iter()
            .find_map(|command| match command {
                goble_ui::render::RenderCommand::DrawIcon { origin, name, size, .. }
                    if name == "trash-02" =>
                {
                    Some((*origin, *size))
                }
                _ => None,
            })
            .expect("the binding row draws its trash icon");
        assert!(press(
            &mut page,
            &app,
            vec2f(origin.x + size / 2.0, origin.y + size / 2.0)
        ));
        assert_eq!(
            dispatched.borrow().as_slice(),
            &[SettingsAction::ShortcutRemap(
                "shortcut.Command palette".to_string()
            )],
            "the trash press is honest: it asks to remap, it does not delete"
        );
    }

    #[test]
    fn the_rail_footer_names_you_and_the_local_workspace() {
        let app = AppContext::default();
        let state = State::default();
        let mut footer = render_rail_footer(&app, &state);
        let commands = render_element(&mut footer, vec2f(200.0, 80.0), &app);
        let drawn = texts(&commands);
        assert!(drawn.iter().any(|text| text == "You"), "{drawn:?}");
        assert!(
            drawn.iter().any(|text| text == "Local workspace"),
            "{drawn:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                goble_ui::render::RenderCommand::DrawText { text, .. } if text == "Y"
            )),
            "the avatar initial comes from the name: {drawn:?}"
        );
    }
}
