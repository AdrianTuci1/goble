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
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, Dialog, Divider, EdgeInsets,
    Element, Expanded, Fill, Flex, Icon, MainAxisAlignment, MainAxisSize, PopupMenu,
    PopupMenuItem, PopupMenuPosition, Scrollable, Spacer, Text, Tooltip, TooltipPosition,
    TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::ui::{SettingsCategory, UiActions, UiSnapshot};

use super::state::SettingsAction;
use super::{pages, render};

/// The widest the floating sheet grows, and the margin it keeps from the window
/// when it can float.
const OVERLAY_MAX_WIDTH: f32 = 1180.0;
const OVERLAY_MARGIN: f32 = 72.0;
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
    let body = render::render_page(app, &model, settings, &actions.settings);
    let scroll = Scrollable::new(body, goble_ui::elements::Axis::Vertical)
        .with_state(settings.scroll.clone())
        .finish();
    let header = overlay_header(app, state, actions, page, narrow, close);
    let page_column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(Divider::horizontal().finish())
        .with_child(Expanded::new(scroll).finish())
        .finish();

    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    if !narrow {
        row = row.with_child(overlay_rail(app, state, actions));
        row = row.with_child(Divider::vertical().finish());
    }
    row = row.with_child(Expanded::new(page_column).finish());
    Container::new(row.finish())
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
    let name = super::fields::app_text(settings, "friendly_name");
    let who = if name.trim().is_empty() { "You".to_string() } else { name };
    let footer = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(4.0)
        .with_child(
            Text::new(who)
                .with_font_size(13.0)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new("Local workspace")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
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
        .with_child(nav)
        .with_child(Spacer::new().finish())
        .with_child(feedback)
        .with_child(Divider::horizontal().finish())
        .with_child(
            Container::new(footer)
                .with_padding(EdgeInsets::new(8.0, 6.0, 8.0, 6.0))
                .finish(),
        )
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

fn overlay_header(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    page: SettingsCategory,
    narrow: bool,
    close: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let mut leading = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0);
    if narrow {
        leading = leading.with_child(hamburger(app, state, actions));
    }
    let labels = if narrow {
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(
                Text::new(page.label())
                    .with_font_size(20.0)
                    .with_line_height(26.0 / 20.0)
                    .with_weight(FontWeight::SemiBold)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                Text::new(page_description(page))
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish()
    } else {
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new(page.label())
                    .with_font_size(20.0)
                    .with_weight(FontWeight::SemiBold)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .finish()
    };
    leading = leading.with_child(labels);
    let close_button = TopbarButton::new(
        Icon::new("x")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(8.0)
    .with_on_click(move || (close.borrow_mut())())
    .finish();
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(leading.finish()).finish())
            .with_child(close_button)
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, md, 0.0, 12.0))
    .finish()
}

fn hamburger(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let pages = SettingsCategory::ALL.to_vec();
    let dispatch = actions.settings.dispatch.clone();
    let trigger = TopbarButton::new(
        Icon::new("menu-01")
            .with_size(15.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(8.0)
    .finish();
    PopupMenu::new(
        trigger,
        pages
            .iter()
            .map(|page| PopupMenuItem::new(page.label()))
            .collect(),
    )
    .with_open(state.settings_overlay_hamburger.clone())
    .with_position(PopupMenuPosition::Below)
    .with_max_width(220.0)
    .with_on_select(move |index| {
        if let Some(page) = pages.get(index).copied() {
            (dispatch.borrow_mut())(SettingsAction::Navigate(page));
        }
    })
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
        _ => "",
    }
}
