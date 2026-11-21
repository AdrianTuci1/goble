//! The Settings overlay: the floating sheet the sidebar's Settings row opens.
//!
//! Wide windows get the T3 sheet — a scrim, a centred panel with its own rail
//! and the file-backed page content. A narrow window drops the rail and puts a
//! hamburger over the same pages, so the sheet can take the whole space without
//! losing navigation. Every row still renders from the same `PageModel` and
//! dispatches through the same `SettingsActions` as the former pane, so the
//! file writes, revision fences and side sheets are unchanged.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::settings::{SettingsNav, SettingsNavGroup, SettingsNavItem};
use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, Dialog, Divider,
    EdgeInsets, Element, Empty, Expanded, Fill, Flex, Icon, MainAxisAlignment, MainAxisSize,
    PopupMenu, PopupMenuItem, PopupMenuPosition, Scrollable, Spacer, Stack, Text, Tooltip,
    TooltipPosition, TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::ui::{SettingsCategory, UiActions, UiSnapshot};

use super::state::SettingsAction;
use super::{pages, render};

/// The widest the floating sheet grows, and the margin it keeps from the window
/// when it can float.
const OVERLAY_MAX_WIDTH: f32 = 1060.0;
const OVERLAY_MARGIN: f32 = 96.0;
/// Below this window width the rail gives way to the hamburger.
const OVERLAY_NARROW: f32 = 740.0;

/// Build the settings overlay. A closed overlay is a zero-size inert dialog.
pub fn build_settings_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    if !state.settings_overlay_open {
        return Dialog::new(goble_ui::elements::Empty::new().finish())
            .with_open(false)
            .finish();
    }
    // The first frame has no measured window yet; draw the wide sheet then,
    // and let the measured narrow width switch to the full-bleed variant.
    let width = state.window_size.x;
    let narrow = !state.settings_overlay_wide;
    let width = if width > 0.0 { width } else { OVERLAY_MAX_WIDTH };
    let on_close = actions.on_close_settings_overlay.clone();
    let close: Rc<RefCell<dyn FnMut()>> =
        Rc::new(RefCell::new(move || (on_close.borrow_mut())()));
    let content = overlay_content(app, state, actions, narrow, close);
    let dialog = Dialog::new(content).with_open(true).with_width(if narrow {
        (width - 16.0).max(320.0)
    } else {
        (width - OVERLAY_MARGIN).min(OVERLAY_MAX_WIDTH)
    });
    if narrow {
        dialog.with_inset(8.0).finish()
    } else {
        dialog.finish()
    }
}

fn overlay_content(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    narrow: bool,
    close: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let settings = &state.settings;
    let page = settings.page;
    let model = pages::page_model(page, settings);
    let body = render::render_page_content(app, &model, settings, &actions.settings);
    let body = if page == SettingsCategory::General {
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Container::new(layout_cards(app, narrow))
                    .with_padding(EdgeInsets::new(24.0, 20.0, 24.0, 0.0))
                    .finish(),
            )
            .with_child(body)
            .finish()
    } else {
        body
    };
    // The floating x sits over the sheet's top-right corner without a bar.
    // Keep the page's first row clear of it: the scroll content starts under a
    // small top band, so the x never covers a control.
    let scroll = Container::new(
        Scrollable::new(body, goble_ui::elements::Axis::Vertical)
            .with_state(settings.scroll.clone())
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, 30.0, 0.0, 0.0))
    .finish();
    // K: no topbar inside the settings content. The page starts directly on
    // its sections; the only close affordance is the floating x over the
    // content's top-right corner, with no bar or rule behind it.
    let close_button = TopbarButton::new(
        Icon::new("x")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(14.0)
    .with_on_click(move || (close.borrow_mut())())
    .finish();
    let close_row = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(Empty::new().finish()).finish())
            .with_child(close_button)
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, 8.0, 12.0, 8.0))
    .finish();
    let mut page_stack = Stack::new().with_children(vec![scroll]);
    // The Appearance popover rides above the scroll viewport (outside its
    // clip), so a palette anchored to a row is never cut by the page edge.
    if let Some(palette) = render::color_palette::appearance_menu_layer(
        app,
        settings,
        &actions.settings,
    ) {
        page_stack = page_stack.with_overlay(palette, vec2f(0.0, 0.0));
    }
    let page_column = page_stack
        .with_overlay(close_row, vec2f(0.0, 0.0))
        .finish();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    if !narrow {
        row = row.with_child(overlay_rail(app, state, actions));
        row = row.with_child(Divider::vertical().finish());
    }
    row = row.with_child(Expanded::new(page_column).finish());
    let row = if narrow {
        row.finish()
    } else {
        ConstrainedBox::new(row.finish())
            .with_height((state.window_size.y * 0.82).max(480.0))
            .finish()
    };
    Container::new(row)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_border(goble_ui::elements::Border::all(1.0)
            .with_border_color(app.theme.color(ColorToken::Border)))
        .with_corner_radius(16.0)
        .finish()
}

fn overlay_rail(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let settings = &state.settings;
    let nav = photo_nav(app, settings, actions);
    let feedback = actions.on_open_url.clone();
    let feedback = goble_ui::elements::HoverRow::new(
        Text::new("Give Feedback")
            .with_font_size(13.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 6.0, 8.0, 6.0))
    .with_corner_radius(8.0)
    .with_on_click(move || {
        (feedback.borrow_mut())("https://github.com/AdrianTuci1/goble/issues".to_string())
    })
    .finish();
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(12.0)
        .with_child(
            Container::new(
                Text::new("Settings")
                    .with_font_size(14.0)
                    .with_weight(FontWeight::SemiBold)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(12.0, 8.0, 12.0, 4.0))
            .finish(),
        )
        .with_child(ConstrainedBox::new(nav).with_height(520.0).finish())
        .with_child(Spacer::new().finish())
        .with_child(feedback)
        .with_child(Divider::horizontal().finish())
        .with_child(render::render_rail_footer(app, settings))
        .finish();
    ConstrainedBox::new(column)
        .with_width(196.0)
        .finish()
}

/// The rail labels and order from the reference photos, mapped onto the real
/// pages this build owns. The last group exposes the remaining file-backed
/// pages (secrets, updates, advanced, about) instead of hiding them.
fn photo_nav(app: &AppContext, settings: &super::SettingsUiState, actions: &UiActions) -> Box<dyn Element> {
    let entries: Vec<(&'static str, SettingsCategory)> = vec![
        ("General", SettingsCategory::General),
        ("Application", SettingsCategory::Agent),
        ("Appearance", SettingsCategory::Appearance),
        ("Shortcuts", SettingsCategory::Shortcuts),
        ("Notifications", SettingsCategory::Updates),
        ("Models", SettingsCategory::Providers),
        ("Browser", SettingsCategory::Connectors),
        ("Projects", SettingsCategory::Environments),
        ("Conversations", SettingsCategory::Hosts),
    ];
    let mut items = Vec::new();
    let mut index = 0usize;
    let mut selected = 0usize;
    for (label, page) in &entries {
        if settings.page == *page {
            selected = index;
        }
        index += 1;
        items.push(SettingsNavItem::new(*label));
    }
    let rest = [
        ("Secrets", SettingsCategory::Secrets),
        ("Updates", SettingsCategory::Updates),
        ("Advanced", SettingsCategory::Advanced),
        ("About", SettingsCategory::About),
    ];
    let mut advanced = SettingsNavGroup::new("Advanced");
    for (label, page) in rest {
        if settings.page == page {
            selected = index;
        }
        index += 1;
        advanced = advanced.with_item(SettingsNavItem::new(label));
    }
    let pages: Vec<SettingsCategory> = entries
        .into_iter()
        .map(|(_, page)| page)
        .chain(rest.into_iter().map(|(_, page)| page))
        .collect();
    let dispatch = actions.settings.dispatch.clone();
    let nav = SettingsNav::new(vec![SettingsNavGroup::new("").with_items(items), advanced])
        .with_selected(selected)
        .with_search(String::new())
        .with_on_select(move |selected| {
            if let Some(page) = pages.get(selected).copied() {
                (dispatch.borrow_mut())(SettingsAction::Navigate(page));
            }
        })
        .finish();
    let _ = app;
    ConstrainedBox::new(nav).with_width(196.0).finish()
}

/// The photo's two Interface Layout tiles: a 2-up card pair with a selected
/// tint on Software Engineering. The preference has no file-backed backend in
/// this build, so the selected state is presented as the product default rather
/// than hidden behind a dead toggle.
fn layout_cards(app: &AppContext, narrow: bool) -> Box<dyn Element> {
    let card = |title: &str, _body: &str, icon: &'static str, selected: bool| -> Box<dyn Element> {
        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(6.0)
            .with_child(
                Container::new(
                    Icon::new(icon)
                        .with_size(18.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(8.0))
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
                .with_corner_radius(10.0)
                .finish(),
            )
            .with_child(
                Text::new(title)
                    .with_font_size(14.0)
                    .with_weight(FontWeight::Medium)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .finish();
        Container::new(content)
            .with_padding(EdgeInsets::uniform(14.0))
            .with_background(Fill::Solid(app.theme.color(if selected {
                ColorToken::Selected
            } else {
                ColorToken::SurfaceRaised
            })))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(if selected {
                ColorToken::Accent
            } else {
                ColorToken::Border
            })))
            .with_corner_radius(12.0)
            .finish()
    };
    let knowledge = card(
        "Knowledge Work",
        "Simplified interface for research, writing, and everyday tasks.",
        "folder",
        false,
    );
    let software = card(
        "Software Engineering",
        "Full interface for coding, developer tools, and managing agents.",
        "code",
        true,
    );
    // A narrow sheet stacks the two tiles full-width: side by side, each card's
    // intrinsic label width would push the pair past the page and clip the
    // second tile (the photo's 2-up is the wide layout).
    let cards: Box<dyn Element> = if narrow {
        Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(12.0)
            .with_child(knowledge)
            .with_child(software)
            .finish()
    } else {
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(16.0)
            .with_child(Expanded::new(knowledge).finish())
            .with_child(Expanded::new(software).finish())
            .finish()
    };
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(12.0)
        .with_child(
            Text::new("Interface Layout")
                .with_font_size(16.0)
                .with_weight(FontWeight::SemiBold)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(cards)
        .finish()
}

fn page_description(page: SettingsCategory) -> &'static str {
    match page {
        SettingsCategory::General => {
            "Configure agent execution, queued message delivery, and permissions."
        }
        SettingsCategory::Shortcuts => "Keyboard shortcuts for quick navigation and control.",
        SettingsCategory::Appearance => "Theme, colours and text scale.",
        SettingsCategory::Providers => "Models and the agent CLIs this machine runs.",
        SettingsCategory::Connectors => "Extensions and MCP servers this workspace can use.",
        SettingsCategory::Environments => "Projects and their environments.",
        SettingsCategory::Hosts => "Saved SSH hosts and the conversations they carry.",
        SettingsCategory::Updates => "Notifications and update status.",
        _ => "",
    }
}
