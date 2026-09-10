//! Environment (medium) controls for the topbar.
//!
//! The sidebar used to host a Local / Remote selector; that moved into the
//! topbar as a compact environment selector (a pill showing the active medium,
//! clickable to switch) plus a "+" menu that opens a new space in a chosen
//! environment, or adds a brand-new environment by name.
//!
//! Driven from [`crate::media::MediaState`] via [`MediaSnapshot`] +
//! [`MediaActions`]. Clicking a medium calls
//! [`crate::media::MediaState::select_medium`] to set the active environment,
//! and the sidebar conversation filter (which reads `selected_medium ->
//! medium_routing`) stays in sync through the snapshot.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element, Fill,
    Flex, Icon, MainAxisAlignment, PopupMenu, PopupMenuItem, Text, TextInput,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::{Dialog, DIALOG_DEFAULT_WIDTH};

use super::{MediaActions, MediaSnapshot, UiActions, UiSnapshot};

/// The topbar's medium controls: a compact environment selector + a "+" menu
/// that opens a new space in a chosen environment (or adds a new medium).
pub fn build_medium_controls(
    app: &AppContext,
    state: &UiSnapshot,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(build_compact_medium_selector(
            app,
            media,
            media_actions,
            state.env_selector_open.clone(),
        ))
        .with_child(build_add_space_menu(app, media, actions, state.add_space_menu_open.clone()))
        .finish()
}

/// A pill showing the active environment; clicking it opens a menu listing the
/// mediums and selects the chosen one (setting the active medium for the next
/// turn via [`crate::media::MediaState::select_medium`]).
fn build_compact_medium_selector(
    app: &AppContext,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
    open: Rc<RefCell<bool>>,
) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let current_label = media
        .mediums
        .iter()
        .find(|m| m.id == media.selected_medium)
        .map(|m| m.label.clone())
        .unwrap_or_else(|| media.selected_medium.clone());

    let trigger = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm * 0.5)
            .with_child(
                Icon::new("computer")
                    .with_size(13.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(current_label)
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                Icon::new("chevron-down")
                    .with_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(sm, md * 0.3, sm, md * 0.3))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(app.theme.radius_px())
    .finish();

    let mut items = Vec::new();
    let mut ids = Vec::new();
    for m in &media.mediums {
        let mut item = PopupMenuItem::new(m.label.clone()).with_icon("computer");
        if m.id == media.selected_medium {
            item = item.selected();
        }
        items.push(item);
        ids.push(m.id.clone());
    }

    let on_select_medium = media_actions.on_select_medium.clone();
    PopupMenu::new(trigger, items)
        .with_open(open)
        .with_on_select(move |index| {
            if let Some(medium_id) = ids.get(index) {
                (on_select_medium.borrow_mut())(medium_id.clone());
            }
        })
        .finish()
}

/// The topbar "+" menu. Pressing "+" opens a warp-new style menu listing the
/// environments; choosing one opens a new space/tab whose default environment is
/// that medium. A trailing "Add new medium…" item opens the add-medium dialog.
fn build_add_space_menu(
    app: &AppContext,
    media: &MediaSnapshot,
    actions: &UiActions,
    open: Rc<RefCell<bool>>,
) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);

    let trigger = Container::new(
        Text::new("+")
            .with_font_size(16.0)
            .with_theme_color(ColorToken::Accent, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(md * 0.5, md * 0.3, md * 0.5, md * 0.3))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(app.theme.radius_px())
    .finish();

    let mut items = Vec::new();
    let mut ids = Vec::new();
    for m in &media.mediums {
        items.push(PopupMenuItem::new(m.label.clone()).with_icon("computer"));
        ids.push(m.id.clone());
    }
    // The trailing "Add new medium…" entry; its index is `ids.len()`.
    items.push(PopupMenuItem::new("Add new medium…").with_icon("plus"));

    let on_add_space_with_medium = actions.on_add_space_with_medium.clone();
    let on_open_add_medium = actions.on_open_add_medium.clone();
    PopupMenu::new(trigger, items)
        .with_open(open)
        .with_on_select(move |index| {
            if index < ids.len() {
                (on_add_space_with_medium.borrow_mut())(ids[index].clone());
            } else {
                // The last item: add a brand-new environment by name.
                (on_open_add_medium.borrow_mut())();
            }
        })
        .finish()
}

/// A centered dialog prompting for a new environment's name. The editable text
/// lives in app-owned state so it survives the per-frame rebuild, and the
/// "Add" button routes to [`MediaActions::on_add_medium`] (which adds the
/// medium, persists it and closes the dialog).
pub fn build_add_medium_dialog(
    app: &AppContext,
    state: &UiSnapshot,
    media_actions: &MediaActions,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let title = Text::new("Add a new environment")
        .with_theme_color(ColorToken::Text, app)
        .with_font_size(16.0)
        .finish();

    let on_draft_change = actions.on_add_medium_draft_change.clone();
    let on_draft_focus = actions.on_add_medium_focus_change.clone();
    let focused = state.add_medium_focused;
    let input = TextInput::new()
        .with_value(state.add_medium_draft.clone())
        .with_placeholder("e.g. Staging VPS")
        .with_focused(focused)
        .with_on_change(move |v| (on_draft_change.borrow_mut())(v))
        .with_on_focus_change(move |is_focused: bool| {
            (on_draft_focus.borrow_mut())(is_focused);
        })
        .finish();

    let on_close = actions.on_add_medium_close.clone();
    let cancel = Button::new(Text::new("Cancel").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_close.borrow_mut())())
        .finish();

    let on_add_medium = media_actions.on_add_medium.clone();
    let draft = state.add_medium_draft.clone();
    let add = Button::new(Text::new("Add").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_add_medium.borrow_mut())(draft.clone()))
        .finish();

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing)
        .with_child(title)
        .with_child(
            Box::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(
                        Text::new("Environment name")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(input),
            ),
        )
        .with_child(
            Box::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::End)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(spacing)
                    .with_child(cancel)
                    .with_child(add),
            ),
        )
        .finish();

    let panel = Container::new(body)
        .with_padding(EdgeInsets::uniform(spacing))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(ColorToken::Border).into())
        .with_corner_radius(app.theme.radius_px())
        .finish();

    let on_close = actions.on_add_medium_close.clone();
    Dialog::new(panel)
        .with_open(state.add_medium_dialog_open)
        .with_width(DIALOG_DEFAULT_WIDTH)
        .with_on_close(move || (on_close.borrow_mut())())
        .finish()
}
