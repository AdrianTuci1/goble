//! The settings tab: a rail of pages beside the page the rail has selected.
//!
//! This module is the visible rework of the old flat settings pane. It is the
//! DSH/T3-shaped surface — a 200px rail, a 48px header band, a centred page with
//! card sections and description rows, and a key-hint footer — built from the
//! shared elements in `goble_ui::elements::settings`.
//!
//! # The file is the source of truth
//!
//! Nothing here is stored in a settings database. The pages render the
//! projection of `~/.goble/config.toml`, `environment.toml` and `hosts.toml`
//! that the desktop service reads through `goble-core::config`, and every edit
//! is a document op through the same editor: comments and unknown keys survive,
//! the write is atomic and mode-0600, and the revision the page rendered from
//! fences the write (a stale one is a conflict, not an overwrite).
//!
//! The wiring is deliberately narrow: `UiState::settings` is one
//! [`SettingsUiState`], the snapshot carries a copy, the actions carry one
//! [`SettingsActions`] dispatch closure, and
//! `apps/desktop/src/actions/settings_actions.rs` is the single place that calls
//! the service. A page is data ([`model::PageModel`]) rendered by [`render`], so
//! the keyboard model and the drawing share one description of each page.
//!
//! # Keyboard
//!
//! The keyboard is in exactly one region — the rail or the page — and the footer
//! says what the keys do there. `↑`/`↓` move within the region, `Enter` moves
//! from the rail into the page or activates the page's focused row, `→` on a
//! page row changes a select or a slider, `Esc` steps out of a field and then
//! back to the rail. `/` puts the caret in the rail's search. A focused text
//! field takes `Enter` (commit) and `Esc` (cancel) and passes every other key to
//! the field, so the caret is never a trap.
//!
//! The focus order is the interactive rows of the page model
//! ([`model::PageModel::interactive_rows`]), the same list the renderer walks
//! while drawing, so a row cannot be drawn that the keys cannot reach.

pub mod fields;
pub mod model;
pub mod pages;
pub mod render;
pub mod state;

pub use model::{PageModel, RowModel, SectionModel};
pub use state::{
    ConnectorForm, HostForm, ModelForm, SettingsAction, SettingsActions, SettingsForm,
    SettingsNotice, SettingsRegion, SettingsUiState, StatusKind,
};

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConstrainedBox, CrossAxisAlignment, Divider, Element, Expanded, Flex, KeyHandler,
    MainAxisAlignment, MainAxisSize, Scrollable, ShortcutHint, ShortcutHints, Text,
};
use goble_ui::color::ColorU;
use goble_ui::event::ModifiersState;
use goble_ui::theme::ColorToken;

use crate::ui::color_picker::ColorTarget;
use crate::ui::{SettingsCategory, UiActions, UiSnapshot};

use self::state::SettingsUiState as State;

/// The settings header band's height: the compact DSH/T3 `h-9` (36pt), down
/// from the 48pt bar the owner asked to shorten. The 28pt buttons fit inside it
/// with no vertical padding.
pub const HEADER_HEIGHT: f32 = 36.0;
/// The footer strip's height.
pub const FOOTER_HEIGHT: f32 = 34.0;

/// Build the settings tab's surface: the rail, the header, the page and the
/// footer, wrapped in one key handler that turns keys into settings actions.
pub fn build_settings_pane(
    app: &AppContext,
    snapshot: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    page: SettingsCategory,
) -> Box<dyn Element> {
    let settings = &snapshot.settings;
    // The header's X (and Escape at the rail) close the settings leaf through
    // the app's own close rule: name the pane, then close it.
    let on_activate = actions.on_pane_activate.clone();
    let on_close = actions.on_close_pane.clone();
    let actions = &actions.settings;
    let close: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || {
        (on_activate.borrow_mut())(pane_id);
        (on_close.borrow_mut())();
    }));
    let model = pages::page_model(page, settings);
    let nav = render::render_nav(app, settings, actions);
    let header = render::render_header(app, settings, actions, page, Rc::clone(&close));
    let body = render::render_page(app, &model, settings, actions);
    let footer = render_footer(app, settings);
    let scroll = Scrollable::new(body, goble_ui::elements::Axis::Vertical)
        .with_state(Rc::clone(&settings.scroll))
        .finish();
    let page_column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        // Stretch, not Center: a Flex column hands non-flex children an
        // unbounded cross axis unless it stretches, and every part of the
        // settings page (the header band, the scroll viewport, the footer) is
        // full width.
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(Expanded::new(scroll).finish())
        .with_child(footer)
        .finish();
    let mut content = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        // Stretch so the page column gets the pane's bounded height: a Flex
        // row hands its children an unbounded cross axis otherwise, and the
        // column would size to infinity and push its footer off screen.
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(nav)
        .with_child(Expanded::new(page_column).finish())
        .finish();
    // The add/edit form, if one is open, is a side sheet docked to the pane's
    // trailing edge. The page behind it is dimmed and a press on it dismisses
    // the sheet (a focus trap, like an open menu's).
    if let Some(sheet_section) = pages::form_section(settings) {
        let sheet = render::render_side_sheet(app, &sheet_section, settings, actions);
        let dismiss_dispatch = actions.dispatch.clone();
        let dismiss: Rc<RefCell<dyn FnMut()>> =
            Rc::new(RefCell::new(move || {
                (dismiss_dispatch.borrow_mut())(SettingsAction::CancelForm);
            }));
        let bg = app.theme.color(ColorToken::Bg);
        let tint = ColorU::new(bg.r, bg.g, bg.b, 150);
        let trapped: Box<dyn Element> = Box::new(render::DismissLayer::new(content, tint, dismiss));
        content = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Expanded::new(trapped).finish())
            .with_child(sheet)
            .finish();
    }
    let surface = goble_ui::elements::Container::new(content)
        .with_background(goble_ui::elements::Fill::Solid(
            app.theme.color(ColorToken::Bg),
        ))
        .finish();
    // The whole surface gets first refusal on keys. A modifier-held key is the
    // app's (`⌘W`, pane navigation), so it is never consumed here.
    let dispatch = actions.dispatch.clone();
    let key_state = settings.clone();
    let key_close = Rc::clone(&close);
    KeyHandler::new(surface, move |key, modifiers| {
        let state = key_state.clone();
        settings_key_with_close(&dispatch, Some(&key_close), &state, key, modifiers)
    })
    .finish()
}

/// The key map as one pure function, so a test can drive it without a window.
pub fn settings_key(
    dispatch: &Rc<RefCell<dyn FnMut(SettingsAction)>>,
    state: &State,
    key: &str,
    modifiers: ModifiersState,
) -> bool {
    settings_key_with_close(dispatch, None, state, key, modifiers)
}

/// [`settings_key`] plus the pane's close affordance: Escape from the rail
/// closes the settings pane (the keyboard equivalent of the header's X), while
/// it keeps stepping a sheet/field or the rail out first.
pub fn settings_key_with_close(
    dispatch: &Rc<RefCell<dyn FnMut(SettingsAction)>>,
    close: Option<&Rc<RefCell<dyn FnMut()>>>,
    state: &State,
    key: &str,
    modifiers: ModifiersState,
) -> bool {
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return false;
    }
    // A side sheet owns the keyboard while it is open: only its own rows step
    // and activate, Escape closes it, and the rail search stays out of reach.
    if state.region == SettingsRegion::Sheet {
        if let Some(field) = state.focused_field.clone() {
            match key {
                "Escape" => {
                    dispatch_action(dispatch, SettingsAction::ClearField { id: field });
                    return true;
                }
                "Enter" => {
                    dispatch_action(dispatch, SettingsAction::CommitField { id: field });
                    return true;
                }
                "ArrowUp" | "ArrowDown" => {
                    dispatch_action(dispatch, SettingsAction::ClearField { id: field });
                    dispatch_action(
                        dispatch,
                        SettingsAction::Step(if key == "ArrowUp" { -1 } else { 1 }),
                    );
                    return true;
                }
                _ => return false,
            }
        }
        return match key {
            "ArrowUp" => {
                dispatch_action(dispatch, SettingsAction::Step(-1));
                true
            }
            "ArrowDown" => {
                dispatch_action(dispatch, SettingsAction::Step(1));
                true
            }
            "Enter" | "Space" | "Tab" => {
                dispatch_action(dispatch, SettingsAction::Activate);
                true
            }
            "Escape" => {
                dispatch_action(dispatch, SettingsAction::Back);
                true
            }
            _ => false,
        };
    }
    // A caret in a field owns printable keys and Backspace; only Escape and
    // Enter are the page's while the field holds the caret, and the field gets
    // everything else through the tree after this handler declines it.
    if let Some(field) = state.focused_field.clone() {
        match key {
            "Escape" => {
                dispatch_action(dispatch, SettingsAction::ClearField { id: field });
                return true;
            }
            "Enter" => {
                dispatch_action(dispatch, SettingsAction::CommitField { id: field });
                return true;
            }
            "ArrowUp" | "ArrowDown" => {
                dispatch_action(dispatch, SettingsAction::ClearField { id: field });
                dispatch_action(
                    dispatch,
                    SettingsAction::Step(if key == "ArrowUp" { -1 } else { 1 }),
                );
                return true;
            }
            _ => return false,
        }
    }
    if state.search_focused {
        match key {
            "Escape" => {
                dispatch_action(dispatch, SettingsAction::FocusSearch(false));
                dispatch_action(dispatch, SettingsAction::ClearSearch);
                return true;
            }
            "Enter" => {
                dispatch_action(dispatch, SettingsAction::Activate);
                return true;
            }
            "ArrowUp" => {
                dispatch_action(dispatch, SettingsAction::Step(-1));
                return true;
            }
            "ArrowDown" => {
                dispatch_action(dispatch, SettingsAction::Step(1));
                return true;
            }
            "Backspace" => {
                dispatch_action(dispatch, SettingsAction::SearchChar("\u{8}".to_string()));
                return true;
            }
            _ => return false,
        }
    }
    match key {
        "ArrowUp" => {
            dispatch_action(dispatch, SettingsAction::Step(-1));
            true
        }
        "ArrowDown" => {
            dispatch_action(dispatch, SettingsAction::Step(1));
            true
        }
        "ArrowLeft" => {
            dispatch_action(dispatch, SettingsAction::Adjust(-1));
            true
        }
        "ArrowRight" => {
            dispatch_action(dispatch, SettingsAction::Adjust(1));
            true
        }
        "Enter" | "Space" | "Tab" => {
            dispatch_action(dispatch, SettingsAction::Activate);
            true
        }
        "Escape" => {
            // From the rail, Escape leaves settings; from a page it steps back
            // to the rail first (the footer names both).
            if state.region == SettingsRegion::Rail {
                if let Some(close) = close {
                    (close.borrow_mut())();
                    return true;
                }
            }
            dispatch_action(dispatch, SettingsAction::Back);
            true
        }
        "/" => {
            dispatch_action(dispatch, SettingsAction::FocusSearch(true));
            true
        }
        _ => false,
    }
}



fn dispatch_action(dispatch: &Rc<RefCell<dyn FnMut(SettingsAction)>>, action: SettingsAction) {
    (dispatch.borrow_mut())(action);
}

fn render_footer(app: &AppContext, settings: &State) -> Box<dyn Element> {
    let mut hints = Vec::new();
    if settings.region == SettingsRegion::Sheet {
        hints.push(ShortcutHint::new(&["↑", "↓"], "fields"));
        hints.push(ShortcutHint::new(&["Enter"], "activate"));
        hints.push(ShortcutHint::new(&["Esc"], "close form"));
    } else if settings.region == SettingsRegion::Rail {
        hints.push(ShortcutHint::new(&["↑", "↓"], "pages"));
        hints.push(ShortcutHint::new(&["Enter"], "open"));
        hints.push(ShortcutHint::new(&["/"], "search"));
    } else {
        hints.push(ShortcutHint::new(&["↑", "↓"], "rows"));
        hints.push(ShortcutHint::new(&["Enter"], "activate"));
        hints.push(ShortcutHint::new(&["←", "→"], "change"));
        hints.push(ShortcutHint::new(&["Esc"], "back"));
    }
    let strip = ShortcutHints::new(hints).finish(app);
    let status = settings
        .status
        .as_ref()
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let status_text = Text::new(status)
        .with_font_size(11.0)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(strip)
        .with_child(status_text)
        .finish();
    let band = ConstrainedBox::new(
        goble_ui::elements::Container::new(row)
            .with_padding(goble_ui::elements::EdgeInsets::new(6.0, 16.0, 6.0, 16.0))
            .finish(),
    )
    .with_min_height(FOOTER_HEIGHT)
    .finish();
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Divider::horizontal().finish())
        .with_child(band)
        .finish()
}

/// The colour-target a theme channel path names, so the page and any future
/// colour picker agree on it.
pub fn channel_target(path: &str) -> Option<ColorTarget> {
    match path.rsplit('.').next()? {
        "accent" => Some(ColorTarget::Accent),
        "primary" => Some(ColorTarget::Primary),
        "secondary" => Some(ColorTarget::Secondary),
        "bg" => Some(ColorTarget::Background),
        "surface" => Some(ColorTarget::Surface),
        _ => None,
    }
}

/// The page's title, used by the header's breadcrumb and by the tests.
pub fn page_title(page: SettingsCategory) -> &'static str {
    page.label()
}

/// The categories in rail order: the same order as [`SettingsCategory::ALL`],
/// grouped for the rail's headings.
pub fn rail_groups() -> Vec<(String, Vec<SettingsCategory>)> {
    use SettingsCategory as C;
    vec![
        (String::new(), vec![C::General, C::Appearance]),
        ("Models".to_string(), vec![C::Providers, C::Connectors]),
        (
            "Agent".to_string(),
            vec![
                C::Agent,
                C::Environments,
                C::Hosts,
                C::Secrets,
                C::Shortcuts,
            ],
        ),
        (
            "System".to_string(),
            vec![C::Updates, C::Advanced, C::About],
        ),
    ]
}

/// Whether `page` has a row whose label or description contains `query` (case
/// insensitive). The rail's search uses this so typing a setting's name shows
/// the page it lives on even when the page's own title is not the query.
pub fn page_matches(page: SettingsCategory, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    if page.label().to_lowercase().contains(&query) {
        return true;
    }
    let schema_page = schema_page(page);
    goble_core::config::FIELDS.iter().any(|spec| {
        schema_page == Some(spec.page)
            && (spec.label.to_lowercase().contains(&query)
                || spec.description.to_lowercase().contains(&query))
    })
}

/// The `goble_core::config::SettingsPage` a rail page corresponds to.
pub fn schema_page(page: SettingsCategory) -> Option<goble_core::config::SettingsPage> {
    use goble_core::config::SettingsPage as P;
    use SettingsCategory as C;
    Some(match page {
        C::General => P::General,
        C::Appearance => P::Appearance,
        C::Providers => P::Providers,
        C::Connectors => P::Connectors,
        C::Agent => P::Agent,
        C::Environments => P::Environments,
        C::Hosts => P::Hosts,
        C::Secrets => P::Secrets,
        C::Shortcuts => P::Shortcuts,
        C::Updates => P::Updates,
        C::Advanced => P::Advanced,
        C::About => P::About,
    })
}
