//! Settings overlay: a centered modal covering the window with a dimmed
//! backdrop, showing the grok-build style categories (Appearance, Mouse,
//! Editor & Input, Agent & Approval, Models, Advanced), each with a few
//! essential controls. Model configuration is done by editing the global
//! `~/.goble/config.toml`; this panel only lists the resulting models and
//! offers a reload.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex, Icon, MainAxisAlignment,
    MainAxisSize, Scrollable, Spacer, Switch, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::color_picker::{ColorPicker, ColorTarget};
use super::{SettingsCategory, UiActions, UiSnapshot, WorkspaceRouting};

const NAV_WIDTH: f32 = 150.0;

/// Build the whole settings overlay (header + nav + content pane).
pub fn build_settings_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let on_close = actions.on_settings_close.clone();
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(
            Text::new("Settings")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(14.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            TopbarButton::new(
                Icon::new("x")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_on_click(move || (on_close.borrow_mut())())
            .finish(),
        )
        .finish();

    let header = Container::new(header)
        .with_padding(EdgeInsets::new(
            0.0,
            app.theme.spacing_px(SpacingToken::Md),
            0.0,
            app.theme.spacing_px(SpacingToken::Md),
        ))
        .finish();

    let nav = build_nav(app, state, actions);
    let pane = build_pane(app, state, actions);

    // Lay the nav (fixed width) + content pane side by side, exactly like the
    // goble-ui SettingsView. The content pane scrolls inside the space left by
    // the header (Appearance alone is taller than the panel), and the body row
    // takes only that leftover height.
    let body = Flex::row()
        .with_main_axis_alignment(MainAxisAlignment::Start)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(ConstrainedBox::new(nav).with_max_width(NAV_WIDTH).finish())
        .with_child(Divider::vertical().finish())
        .with_child(
            Expanded::new(
                Scrollable::new(pane, Axis::Vertical)
                    .with_state(state.settings_scroll.clone())
                    .finish(),
            )
            .finish(),
        );

    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(header)
        .with_child(Divider::horizontal().finish())
        .with_child(Expanded::new(body.finish()).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

/// Left column listing each settings category; the active one is highlighted.
fn build_nav(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(sm);

    for &cat in SettingsCategory::ALL {
        let on_select = actions.on_settings_category.clone();
        let selected = cat == state.settings_category;
        let label = Text::new(cat.label())
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(12.0)
            .finish();
        let button = Button::new(label)
            .with_variant(if selected { ButtonVariant::Primary } else { ButtonVariant::Ghost })
            .with_on_click(move || (on_select.borrow_mut())(cat))
            .finish();
        col = col.with_child(Container::new(button).with_padding_uniform(sm).finish());
    }

    Container::new(col.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::new(0.0, md, 0.0, md))
        .finish()
}

/// Content pane for the active category.
fn build_pane(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    match state.settings_category {
        SettingsCategory::Appearance => build_appearance(app, state, actions),
        SettingsCategory::Mouse => build_mouse(app, state, actions),
        SettingsCategory::EditorInput => build_editor(app, state, actions),
        SettingsCategory::AgentApproval => build_agent(app, state, actions),
        SettingsCategory::Models => build_models(app, state, actions),
        SettingsCategory::Advanced => build_advanced(app, state, actions),
    }
}

/// A row with a text label on the left and a `Switch` on the right.
fn switch_row(
    app: &AppContext,
    label: &str,
    checked: bool,
    on_change: Rc<RefCell<dyn FnMut(bool)>>,
) -> Box<dyn Element> {
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Switch::new()
                .with_checked(checked)
                .with_on_change(move |v| (on_change.borrow_mut())(v))
                .finish(),
        )
        .finish();
    Container::new(row).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish()
}

/// A row with a label and a `- value +` stepper.
fn stepper_row(
    app: &AppContext,
    label: &str,
    value: impl ToString,
    on_dec: Rc<RefCell<dyn FnMut()>>,
    on_inc: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let minus = Button::new(Text::new("-").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_dec.borrow_mut())())
        .finish();
    let plus = Button::new(Text::new("+").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_inc.borrow_mut())())
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(minus)
        .with_child(
            Text::new(value.to_string())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(plus)
        .finish();
    Container::new(row).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish()
}

fn build_appearance(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let row = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(sm)
        .with_child(build_color_picker(app, state, actions, ColorTarget::Primary))
        .with_child(build_color_picker(app, state, actions, ColorTarget::Secondary))
        .with_child(build_color_picker(app, state, actions, ColorTarget::Accent));
    let col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(sm)
        .with_child(switch_row(
            app,
            "Dark mode",
            state.settings_dark_mode,
            actions.on_toggle_dark_mode.clone(),
        ))
        .with_child(Divider::horizontal().finish())
        .with_child(row.finish());
    Container::new(col.finish()).finish()
}

/// Resolve the effective color for a theme channel: the custom override if set,
/// else the theme's built-in token color.
fn effective_theme_color(
    app: &AppContext,
    state: &UiSnapshot,
    target: ColorTarget,
) -> ColorU {
    let hex = match target {
        ColorTarget::Primary => state.theme_primary.as_deref(),
        ColorTarget::Secondary => state.theme_secondary.as_deref(),
        ColorTarget::Accent => state.theme_accent.as_deref(),
    };
    if let Some(c) = hex.and_then(ColorU::from_hex) {
        return c;
    }
    let token = match target {
        ColorTarget::Primary => ColorToken::Text,
        ColorTarget::Secondary => ColorToken::Muted,
        ColorTarget::Accent => ColorToken::Accent,
    };
    app.theme.color(token)
}

/// A single theme color: a labeled live swatch + the HSV picker that edits it.
fn build_color_picker(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    target: ColorTarget,
) -> Box<dyn Element> {
    let label = match target {
        ColorTarget::Primary => "Primary",
        ColorTarget::Secondary => "Secondary",
        ColorTarget::Accent => "Accent",
    };
    let on_change = match target {
        ColorTarget::Primary => actions.on_set_theme_primary.clone(),
        ColorTarget::Secondary => actions.on_set_theme_secondary.clone(),
        ColorTarget::Accent => actions.on_set_theme_accent.clone(),
    };
    let color = effective_theme_color(app, state, target);
    // The current hex, shown under the picker so the user can read it back.
    let hex = color.to_hex_string();

    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        // Live swatch of the current color.
        .with_child(
            ConstrainedBox::new(
                Container::new(Box::new(Empty::new()))
                    .with_background(Fill::Solid(color))
                    .finish(),
            )
            .with_width(18.0)
            .with_height(18.0)
            .finish(),
        )
        .finish();

    let picker = ColorPicker::new(color, target)
        .with_drag(state.theme_color_drag.clone())
        .with_on_change(move |c: goble_ui::color::ColorU| {
            // Report the new color as `#rrggbb` to set + persist the channel.
            let hex = c.to_hex_string();
            (on_change.borrow_mut())(hex);
        })
        .finish();

    let block = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Xs))
        .with_child(header)
        .with_child(picker)
        .with_child(Text::new(hex).with_theme_color(ColorToken::Muted, app).with_font_size(11.0).finish())
        .finish();
    Container::new(block)
        .with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm))
        .finish()
}

fn build_mouse(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_dec = {
        let a = actions.on_set_scroll_speed.clone();
        let current = state.settings_scroll_speed;
        Rc::new(RefCell::new(move || (a.borrow_mut())(current - 1)))
    };
    let on_inc = {
        let a = actions.on_set_scroll_speed.clone();
        let current = state.settings_scroll_speed;
        Rc::new(RefCell::new(move || (a.borrow_mut())(current + 1)))
    };
    let rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(switch_row(
            app,
            "Invert scroll",
            state.settings_invert_scroll,
            actions.on_toggle_invert_scroll.clone(),
        ))
        .with_child(stepper_row(
            app,
            "Scroll speed",
            state.settings_scroll_speed,
            on_dec,
            on_inc,
        ))
        .finish();
    Container::new(rows).finish()
}

fn build_editor(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_dec = {
        let a = actions.on_set_font_size.clone();
        let current = state.settings_font_size;
        Rc::new(RefCell::new(move || (a.borrow_mut())(current - 0.1)))
    };
    let on_inc = {
        let a = actions.on_set_font_size.clone();
        let current = state.settings_font_size;
        Rc::new(RefCell::new(move || (a.borrow_mut())(current + 0.1)))
    };
    let hint = Text::new("Use Cmd/Ctrl+Plus/Minus to zoom, or adjust here.")
        .with_theme_color(ColorToken::Muted, app)
        .with_font_size(11.0)
        .finish();
    let rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(stepper_row(
            app,
            "Font size",
            format!("{:.1}x", state.settings_font_size),
            on_dec,
            on_inc,
        ))
        .with_child(Container::new(hint).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish())
        .finish();
    Container::new(rows).finish()
}

fn build_agent(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_local = actions.on_choose_workspace.clone();
    let on_remote = actions.on_choose_workspace.clone();
    let routing = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(
            Text::new("Run agent")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Button::new(Text::new("Local").finish())
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (on_local.borrow_mut())(WorkspaceRouting::Local))
                .finish(),
        )
        .with_child(
            Button::new(Text::new("Remote").finish())
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (on_remote.borrow_mut())(WorkspaceRouting::Remote))
                .finish(),
        )
        .finish();

    // The global auto-approve switch edits the active pane's control (the
    // setting is stored per pane and mirrored into the shared setting).
    let pane_id = state.active_pane_id;
    let on_auto_approve_action = actions.on_toggle_auto_approve.clone();
    let on_auto_approve: Rc<RefCell<dyn FnMut(bool)>> =
        Rc::new(RefCell::new(move |v: bool| {
            (on_auto_approve_action.borrow_mut())(pane_id, v)
        }));
    let rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(switch_row(
            app,
            "Auto-approve",
            state.auto_approve,
            on_auto_approve,
        ))
        .with_child(Container::new(routing).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish())
        .finish();
    Container::new(rows).finish()
}

fn build_models(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm));

    col = col.with_child(
        Text::new("Models (from ~/.goble/config.toml)")
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(12.0)
            .finish(),
    );
    if state.models.is_empty() {
        col = col.with_child(
            Text::new("No models configured yet.")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        );
    }
    for model in &state.models {
        let selected = *model == state.selected_model;
        col = col.with_child(
            Text::new(if selected {
                format!("{model}  ✓")
            } else {
                model.clone()
            })
            .with_theme_color(if selected { ColorToken::Accent } else { ColorToken::Text }, app)
            .with_font_size(12.0)
            .finish(),
        );
    }

    let on_reload = actions.on_reload_model_config.clone();
    let reload = Button::new(Text::new("Reload from config").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_reload.borrow_mut())())
        .finish();

    col = col
        .with_child(Divider::horizontal().finish())
        .with_child(Container::new(reload).with_padding_uniform(md).finish());

    Container::new(col.finish()).finish()
}

fn build_advanced(app: &AppContext, _state: &UiSnapshot, _actions: &UiActions) -> Box<dyn Element> {
    let ssh = ssh_configured();
    let note = Text::new(if ssh {
        "SSH keys: configured (~/.ssh)"
    } else {
        "SSH keys: none found (~/.ssh)"
    })
    .with_theme_color(ColorToken::Text, app)
    .with_font_size(12.0)
    .finish();

    let config_note = Text::new("Config file: ~/.goble/config.toml")
        .with_theme_color(ColorToken::Muted, app)
        .with_font_size(11.0)
        .finish();

    let col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(Container::new(note).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish())
        .with_child(Container::new(config_note).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish())
        .finish();
    Container::new(col).finish()
}

/// Whether a private key exists under `~/.ssh` (e.g. `id_*`).
fn ssh_configured() -> bool {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let ssh = std::path::Path::new(&home).join(".ssh");
    std::fs::read_dir(&ssh)
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with("id_")
            })
        })
        .unwrap_or(false)
}


