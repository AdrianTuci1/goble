//! Settings overlay: a wide panel inset a few pixels from the window edges,
//! showing the grok-build style categories (Appearance, Mouse, Editor & Input,
//! Agent & Approval, Environment, Models, Advanced), each with a few essential
//! controls. Model configuration is done by editing the global
//! `~/.goble/config.toml`; this panel only lists the resulting models and
//! offers a reload.
//!
//! # Keyboard model
//!
//! The whole panel is navigable from the keyboard, not just the category rail.
//! There are **two focus regions** — the rail and the content pane — and the
//! keyboard is in exactly one of them. `UiState::settings_focus` says which,
//! `UiState::settings_pane_focus` says which pane control the keyboard is on,
//! and `UiState::settings_pane_field_active` says whether that control is a
//! text field the caret has been put into. All three live in app state and
//! reach this module through the per-frame [`UiSnapshot`], so the selection
//! survives the element rebuild.
//!
//! Modifier-held keys are never the overlay's: `Ctrl`/`Cmd`/`Alt` plus an
//! arrow is the app's pane navigation (`RootView::dispatch_event`), so those
//! fall through untouched, exactly as they did before.
//!
//! | Key | Rail | Pane |
//! | --- | --- | --- |
//! | `Up` / `Down` | move the category, wrapping | move the pane's focused control, clamped at the ends |
//! | `Right` or `Enter` / `Tab` / `Space` | into the pane, on its first control | adjust the focused control forward if it holds a discrete value; otherwise nothing |
//! | `Left` | nothing (the rail is the leftmost region) | adjust the focused control back if it holds a discrete value; otherwise back to the rail |
//! | `Enter` / `Space` | into the pane | activate the control: a switch flips, a button runs, a text field takes the caret |
//! | `Esc` | close the overlay | back to the rail |
//!
//! Opening the overlay starts in the rail, and changing the category resets
//! the pane's focus to that pane's first control.
//!
//! **Text fields.** A field only *holds the caret* once Enter/Space has put it
//! there; until then it is just the focused control, drawn with the ring.
//! While a field holds the caret the overlay reserves **`Escape` alone** —
//! everything else, printable keys, `Space` and `Backspace` included, must
//! reach the field — so `Escape` is what leaves it. `Escape` steps out one
//! level at a time: out of the field (the caret goes, the ring stays), then
//! out of the pane, then out of the overlay. A field the user typed into is
//! therefore never a trap.
//!
//! The pane's focus order is [`pane_controls`]: every interactive row the pane
//! draws, in the order it draws it. Each pane walks that list while it builds
//! its rows through [`FocusOrder`], which asserts the row being drawn is the
//! one the order holds at that slot and that the pane left nothing undrawn —
//! so a control cannot be added to a pane and silently skipped by the keys.
//!
//! The focused control is marked with [`ColorToken::Focus`], the same blue the
//! focused text field's ring and caret use.

use std::cell::RefCell;
use std::rc::Rc;

use goble_core::store::{SecretEntry, SecretGroup};
use goble_ui::color::ColorU;
use goble_ui::elements::{
    AppContext, Axis, Border, Button, ButtonVariant, ConstrainedBox, Container,
    CrossAxisAlignment, Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex, HoverRow,
    Icon, KeyHandler, MainAxisAlignment, MainAxisSize, Scrollable, Spacer, Switch, Text,
    TextInput, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::color_picker::{ColorTarget, ColorWheel, COLOR_TARGET_ORDER};
use super::{
    SettingsCategory, SettingsControl, SettingsFocus, UiActions, UiSnapshot, WorkspaceRouting,
};

/// Width of the category rail on the left of the panel.
const NAV_WIDTH: f32 = 180.0;
/// Widest the content pane grows; a wide panel should not stretch a short row
/// of controls across the whole window.
const CONTENT_MAX_WIDTH: f32 = 720.0;

/// The focus order of one category's pane: every interactive row it draws, in
/// the order it draws them.
///
/// This is the one list both sides agree on — the pane walks it while building
/// its rows, and the keyboard resolves the focused index through it — so the
/// two cannot drift.
pub fn pane_controls(
    category: SettingsCategory,
    groups: &[SecretGroup],
    open_group: Option<&str>,
) -> Vec<SettingsControl> {
    use SettingsControl::*;
    match category {
        SettingsCategory::Appearance => {
            let mut controls = vec![DarkMode];
            controls.extend(COLOR_TARGET_ORDER.iter().copied().map(ThemeChannel));
            controls.push(ColorWheel);
            controls
        }
        SettingsCategory::Mouse => vec![InvertScroll, ScrollSpeed],
        SettingsCategory::EditorInput => vec![FontSize],
        SettingsCategory::AgentApproval => vec![
            AutoApprove,
            VimMode,
            Route(WorkspaceRouting::Local),
            Route(WorkspaceRouting::Remote),
        ],
        SettingsCategory::Models => vec![ReloadModels],
        // Nothing in Advanced is interactive: the pane reports the SSH and
        // config-file state and nothing else.
        SettingsCategory::Advanced => Vec::new(),
        SettingsCategory::Environment => {
            let mut controls = vec![EnvironmentGroupName, EnvironmentCreateGroup];
            for group in groups {
                controls.push(EnvironmentGroup(group.id.clone()));
                controls.push(EnvironmentDeleteGroup(group.id.clone()));
            }
            if let Some(open) = open_group.and_then(|id| groups.iter().find(|g| g.id == id)) {
                controls.push(EnvironmentSecretName);
                controls.push(EnvironmentSecretValue);
                controls.push(EnvironmentSaveSecret);
                for entry in &open.entries {
                    controls.push(EnvironmentSecret(entry.id.clone()));
                    controls.push(EnvironmentDeleteSecret(entry.id.clone()));
                }
            }
            controls
        }
    }
}

/// The pane slot the keyboard is on, when the pane has the keyboard.
fn pane_focus(state: &UiSnapshot) -> Option<usize> {
    (state.settings_focus == SettingsFocus::Pane).then_some(state.settings_pane_focus)
}

/// Walk a pane's focus order while building its rows.
///
/// `take` claims the next slot and hands back whether the keyboard is on it;
/// it asserts the row being drawn is the control the order says belongs there,
/// and `finish` asserts the pane drew every slot. A pane that drops a row, or
/// draws one the order does not have, fails in a test instead of leaving a
/// control the keys reach but nothing draws (or the other way round).
struct FocusOrder<'a> {
    controls: &'a [SettingsControl],
    focus: Option<usize>,
    at: usize,
}

impl<'a> FocusOrder<'a> {
    fn new(state: &UiSnapshot, controls: &'a [SettingsControl]) -> Self {
        Self {
            controls,
            focus: pane_focus(state),
            at: 0,
        }
    }

    /// Claim the slot for `control`; `true` when the keyboard is on it.
    fn take(&mut self, control: SettingsControl) -> bool {
        debug_assert!(
            self.controls.get(self.at) == Some(&control),
            "the pane drew {control:?} where the focus order has {:?}",
            self.controls.get(self.at)
        );
        let focused = self.focus == Some(self.at);
        self.at += 1;
        focused
    }

    /// Assert the pane drew every slot of its order.
    fn finish(&self) {
        debug_assert_eq!(
            self.at,
            self.controls.len(),
            "the pane left {:?} undrawn",
            &self.controls[self.at.min(self.controls.len())..]
        );
    }
}

/// Mark a control as the pane's focused one with a ring in `ColorToken::Focus`
/// — the colour the focused text field's own ring already uses. An unfocused
/// control is returned unwrapped, so nothing else about the tree changes.
fn focus_ring(app: &AppContext, element: Box<dyn Element>, focused: bool) -> Box<dyn Element> {
    if !focused {
        return element;
    }
    Container::new(element)
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Focus)))
        .with_corner_radius(app.theme.radius_px())
        .finish()
}

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
            Text::new("Arrows move · → into the page · Esc back/close")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        )
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
    // the header (Appearance alone is taller than the panel), keeps a readable
    // max width, and the body row takes only that leftover height.
    let md = app.theme.spacing_px(SpacingToken::Md);
    let content = ConstrainedBox::new(
        Container::new(pane)
            .with_padding(EdgeInsets::new(md, md, md, md))
            .finish(),
    )
    .with_max_width(CONTENT_MAX_WIDTH)
    .finish();
    let body = Flex::row()
        .with_main_axis_alignment(MainAxisAlignment::Start)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(ConstrainedBox::new(nav).with_max_width(NAV_WIDTH).finish())
        .with_child(Divider::vertical().finish())
        .with_child(
            Expanded::new(
                Scrollable::new(content, Axis::Vertical)
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

    let panel = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish();

    // The overlay sees every key before its children do. Modified arrows are
    // the app's pane navigation, so they are never the panel's. Which of the
    // remaining keys the panel keeps depends on where the keyboard is — see
    // the module comment; the short of it is that a text field holding the
    // caret keeps everything but `Escape`.
    let on_step = actions.on_settings_focus_move.clone();
    let on_into_pane = actions.on_settings_focus_into_pane.clone();
    let on_out_of_pane = actions.on_settings_focus_out_of_pane.clone();
    let on_activate = actions.on_settings_activate.clone();
    let on_adjust = actions.on_settings_adjust.clone();
    let on_release_field = actions.on_settings_release_field.clone();
    let on_escape = actions.on_settings_close.clone();
    let focus = state.settings_focus;
    let field_active = state.settings_pane_field_active;
    let focused_control = pane_controls(
        state.settings_category,
        &state.settings_environment_groups,
        state.settings_environment_open_group.as_deref(),
    )
    .get(state.settings_pane_focus)
    .cloned();
    KeyHandler::new(panel, move |key: &str, modifiers| {
        if modifiers.ctrl || modifiers.command || modifiers.alt {
            return false;
        }
        if field_active {
            // Only `Escape` is the overlay's while a field holds the caret;
            // letters, `Space` and `Backspace` go to the field.
            if key == "Escape" {
                (on_release_field.borrow_mut())();
                return true;
            }
            return false;
        }
        let adjust = focused_control
            .as_ref()
            .is_some_and(SettingsControl::has_discrete_values);
        match (focus, key) {
            (_, "Escape") => match focus {
                SettingsFocus::Rail => (on_escape.borrow_mut())(),
                SettingsFocus::Pane => (on_out_of_pane.borrow_mut())(),
            },
            (_, "ArrowUp") => (on_step.borrow_mut())(-1),
            (_, "ArrowDown") => (on_step.borrow_mut())(1),
            // `Left` walks back out of the pane unless the focused control has
            // a discrete value of its own to walk back through.
            (SettingsFocus::Pane, "ArrowLeft") => {
                if adjust {
                    (on_adjust.borrow_mut())(-1);
                } else {
                    (on_out_of_pane.borrow_mut())();
                }
            }
            (SettingsFocus::Rail, "ArrowLeft") => return false,
            (SettingsFocus::Rail, "ArrowRight" | "Enter" | "Tab" | " ") => {
                (on_into_pane.borrow_mut())()
            }
            (SettingsFocus::Pane, "ArrowRight") => {
                if adjust {
                    (on_adjust.borrow_mut())(1);
                } else {
                    return false;
                }
            }
            (SettingsFocus::Pane, "Enter" | "Tab" | " ") => (on_activate.borrow_mut())(),
            _ => return false,
        }
        true
    })
    .finish()
}

/// Left column listing each settings category; the active one is highlighted
/// and, while the rail holds the keyboard, ringed in the focus colour.
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
        let focused = selected && state.settings_focus == SettingsFocus::Rail;
        col = col.with_child(
            Container::new(focus_ring(app, button, focused))
                .with_padding_uniform(sm)
                .finish(),
        );
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
        SettingsCategory::Environment => build_environment(app, state, actions),
        SettingsCategory::Models => build_models(app, state, actions),
        SettingsCategory::Advanced => build_advanced(app, state, actions),
    }
}

/// The pane's focus order for the category the overlay is showing, ready to be
/// walked while the pane is built.
fn order_for(state: &UiSnapshot) -> Vec<SettingsControl> {
    pane_controls(
        state.settings_category,
        &state.settings_environment_groups,
        state.settings_environment_open_group.as_deref(),
    )
}

/// A row with a text label on the left and a `Switch` on the right.
fn switch_row(
    app: &AppContext,
    label: &str,
    checked: bool,
    on_change: Rc<RefCell<dyn FnMut(bool)>>,
    focused: bool,
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
    Container::new(focus_ring(app, row, focused))
        .with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm))
        .finish()
}

/// A row with a label and a `- value +` stepper.
fn stepper_row(
    app: &AppContext,
    label: &str,
    value: impl ToString,
    on_dec: Rc<RefCell<dyn FnMut()>>,
    on_inc: Rc<RefCell<dyn FnMut()>>,
    focused: bool,
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
    Container::new(focus_ring(app, row, focused))
        .with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm))
        .finish()
}

fn build_appearance(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);
    // A slot is claimed in the order the list holds, not in the order the rows
    // happen to be built, so the appearance pane takes its switch first even
    // though the wheel block is built above it.
    let dark_focused = order.take(SettingsControl::DarkMode);

    let heading = Text::new("Theme colors")
        .with_theme_color(ColorToken::Muted, app)
        .with_font_size(11.0)
        .finish();

    let target = state.theme_color_target;
    let color = effective_theme_color(app, state, target);
    let on_change = match target {
        ColorTarget::Primary => actions.on_set_theme_primary.clone(),
        ColorTarget::Secondary => actions.on_set_theme_secondary.clone(),
        ColorTarget::Accent => actions.on_set_theme_accent.clone(),
    };
    let wheel = ColorWheel::new(color)
        .with_drag(state.theme_color_drag.clone())
        .with_on_change(move |c: ColorU| {
            // Report the new color as `#rrggbb` to set + persist the channel.
            (on_change.borrow_mut())(c.to_hex_string());
        })
        .finish();

    // The wheel edits one channel at a time: this column picks which.
    let wheel_block = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(md)
        .with_child(build_color_targets(app, state, actions, &mut order))
        .with_child(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(app.theme.spacing_px(SpacingToken::Xs))
                .with_child(focus_ring(
                    app,
                    wheel,
                    order.take(SettingsControl::ColorWheel),
                ))
                .with_child(
                    Text::new(format!("{}  {}", target.label(), color.to_hex_string()))
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(11.0)
                        .finish(),
                )
                .finish(),
        )
        .finish();

    let col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(sm)
        .with_child(switch_row(
            app,
            "Dark mode",
            state.settings_dark_mode,
            actions.on_toggle_dark_mode.clone(),
            dark_focused,
        ))
        .with_child(Divider::horizontal().finish())
        .with_child(Container::new(heading).with_padding_uniform(sm).finish())
        .with_child(Container::new(wheel_block).with_padding_uniform(sm).finish());
    order.finish();
    Container::new(col.finish()).finish()
}

/// The three theme channels as clickable swatches; the active one is the
/// channel the wheel edits, and its swatch is highlighted.
fn build_color_targets(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    order: &mut FocusOrder<'_>,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(app.theme.spacing_px(SpacingToken::Xs));

    for target in COLOR_TARGET_ORDER {
        let focused = order.take(SettingsControl::ThemeChannel(target));
        let selected = target == state.theme_color_target;
        let color = effective_theme_color(app, state, target);
        let on_select = actions.on_set_theme_target.clone();
        let swatch = ConstrainedBox::new(
            Container::new(Box::new(Empty::new()))
                .with_background(Fill::Solid(color))
                .finish(),
        )
        .with_width(16.0)
        .with_height(16.0)
        .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(sm)
            .with_child(
                Text::new(target.label())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(swatch)
            .finish();
        let button = Button::new(row)
            .with_variant(if selected { ButtonVariant::Primary } else { ButtonVariant::Ghost })
            .with_on_click(move || (on_select.borrow_mut())(target))
            .finish();
        col = col.with_child(focus_ring(app, button, focused));
    }

    ConstrainedBox::new(col.finish()).with_width(150.0).finish()
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

fn build_mouse(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

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
            order.take(SettingsControl::InvertScroll),
        ))
        .with_child(stepper_row(
            app,
            "Scroll speed",
            state.settings_scroll_speed,
            on_dec,
            on_inc,
            order.take(SettingsControl::ScrollSpeed),
        ))
        .finish();
    order.finish();
    Container::new(rows).finish()
}

fn build_editor(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

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
            order.take(SettingsControl::FontSize),
        ))
        .with_child(Container::new(hint).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish())
        .finish();
    order.finish();
    Container::new(rows).finish()
}

fn build_agent(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

    let on_local = actions.on_choose_workspace.clone();
    let on_remote = actions.on_choose_workspace.clone();
    // Slots are claimed in the order the list holds (the switches come before
    // the routing buttons), not the order the rows happen to be built in.
    let auto_approve_focused = order.take(SettingsControl::AutoApprove);
    let vim_focused = order.take(SettingsControl::VimMode);
    let local_focused = order.take(SettingsControl::Route(WorkspaceRouting::Local));
    let remote_focused = order.take(SettingsControl::Route(WorkspaceRouting::Remote));
    let local = Button::new(Text::new("Local").finish())
        .with_variant(if state.workspace_routing == Some(WorkspaceRouting::Local) {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Ghost
        })
        .with_on_click(move || (on_local.borrow_mut())(WorkspaceRouting::Local))
        .finish();
    let remote = Button::new(Text::new("Remote").finish())
        .with_variant(if state.workspace_routing == Some(WorkspaceRouting::Remote) {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Ghost
        })
        .with_on_click(move || (on_remote.borrow_mut())(WorkspaceRouting::Remote))
        .finish();
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
        .with_child(focus_ring(app, local, local_focused))
        .with_child(focus_ring(app, remote, remote_focused))
        .finish();

    // The global auto-approve switch edits the active pane's control (the
    // setting is stored per pane and mirrored into the shared setting).
    let pane_id = state.active_pane_id;
    let on_auto_approve_action = actions.on_toggle_auto_approve.clone();
    let on_auto_approve: Rc<RefCell<dyn FnMut(bool)>> =
        Rc::new(RefCell::new(move |v: bool| {
            (on_auto_approve_action.borrow_mut())(pane_id, v)
        }));
    // Modal editing is a single app-wide choice (the pane states are per pane,
    // but the switch that turns the feature on is not).
    let on_vim_action = actions.on_toggle_vim_mode.clone();
    let on_vim: Rc<RefCell<dyn FnMut(bool)>> = Rc::new(RefCell::new(move |v: bool| {
        (on_vim_action.borrow_mut())(v)
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
            auto_approve_focused,
        ))
        .with_child(switch_row(
            app,
            "Vim mode",
            state.vim_mode,
            on_vim,
            vim_focused,
        ))
        .with_child(Container::new(routing).with_padding_uniform(app.theme.spacing_px(SpacingToken::Sm)).finish())
        .finish();
    order.finish();
    Container::new(rows).finish()
}

/// Settings → Environment: the groups of secrets, and the group that is open.
///
/// A group is created by name, listed, opened (its secrets then shown), and
/// deleted with everything in it. Inside an open group a secret is added from
/// the two fields, loaded back into them for editing by activating its row, and
/// removed on its own. Every row here is a slot in the pane's focus order, so
/// the whole surface works from the keyboard.
fn build_environment(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let groups = &state.settings_environment_groups;
    let open_group = state
        .settings_environment_open_group
        .as_deref()
        .and_then(|id| groups.iter().find(|g| g.id == id));
    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_spacing(sm);

    col = col.with_child(
        Text::new("Groups of secrets. They are stored in the local database, unencrypted, for a turn that runs remotely.")
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(11.0)
            .finish(),
    );

    // New group: a name field and the button that creates it.
    let group_field_focused = order.take(SettingsControl::EnvironmentGroupName);
    let on_group_draft = actions.on_environment_group_draft_change.clone();
    let group_field = TextInput::new()
        .with_value(state.settings_environment_group_draft.clone())
        .with_placeholder("New group name, e.g. production")
        .with_focused(group_field_focused && state.settings_pane_field_active)
        .with_on_change(move |value| (on_group_draft.borrow_mut())(value))
        .finish();
    let create_focused = order.take(SettingsControl::EnvironmentCreateGroup);
    let on_create = actions.on_environment_create_group.clone();
    let create = Button::new(Text::new("Create group").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_create.borrow_mut())())
        .finish();
    col = col.with_child(
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(Expanded::new(focus_ring(app, group_field, group_field_focused)).finish())
                .with_child(focus_ring(app, create, create_focused))
                .finish(),
        )
        .with_padding_uniform(sm)
        .finish(),
    );

    col = col.with_child(
        Container::new(
            Text::new("Groups")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        )
        .with_padding_uniform(sm)
        .finish(),
    );

    if groups.is_empty() {
        col = col.with_child(
            Container::new(
                Text::new("No groups yet. Name one above to start.")
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_padding_uniform(sm)
            .finish(),
        );
    }
    for group in groups {
        let row_focused = order.take(SettingsControl::EnvironmentGroup(group.id.clone()));
        let delete_focused =
            order.take(SettingsControl::EnvironmentDeleteGroup(group.id.clone()));
        col = col.with_child(build_group_row(
            app,
            group,
            open_group.map(|g| g.id.as_str()) == Some(group.id.as_str()),
            actions,
            row_focused,
            delete_focused,
        ));
    }

    if let Some(group) = open_group {
        col = col.with_child(Divider::horizontal().finish());
        col = col.with_child(
            Container::new(
                Text::new(format!("Secrets in {}", group.name))
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_padding_uniform(sm)
            .finish(),
        );

        let name_focused = order.take(SettingsControl::EnvironmentSecretName);
        let on_name = actions.on_environment_secret_name_change.clone();
        let name_field = TextInput::new()
            .with_value(state.settings_environment_secret_name.clone())
            .with_placeholder("Secret name, e.g. API_KEY")
            .with_focused(name_focused && state.settings_pane_field_active)
            .with_on_change(move |value| (on_name.borrow_mut())(value))
            .finish();
        let value_focused = order.take(SettingsControl::EnvironmentSecretValue);
        let on_value = actions.on_environment_secret_value_change.clone();
        let value_field = TextInput::new()
            .with_value(state.settings_environment_secret_value.clone())
            .with_placeholder("Secret value")
            .with_focused(value_focused && state.settings_pane_field_active)
            .with_on_change(move |value| (on_value.borrow_mut())(value))
            .finish();
        let save_focused = order.take(SettingsControl::EnvironmentSaveSecret);
        let on_save = actions.on_environment_save_secret.clone();
        let save = Button::new(Text::new(if state.settings_environment_editing.is_some() {
            "Save secret"
        } else {
            "Add secret"
        })
        .finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_save.borrow_mut())())
        .finish();

        col = col.with_child(
            Container::new(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(sm)
                    .with_child(focus_ring(app, name_field, name_focused))
                    .with_child(focus_ring(app, value_field, value_focused))
                    .with_child(focus_ring(app, save, save_focused))
                    .finish(),
            )
            .with_padding_uniform(sm)
            .finish(),
        );

        if group.entries.is_empty() {
            col = col.with_child(
                Container::new(
                    Text::new("No secrets in this group yet.")
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .with_padding_uniform(sm)
                .finish(),
            );
        }
        for entry in &group.entries {
            let entry_focused = order.take(SettingsControl::EnvironmentSecret(entry.id.clone()));
            let delete_focused =
                order.take(SettingsControl::EnvironmentDeleteSecret(entry.id.clone()));
            col = col.with_child(build_secret_row(
                app,
                entry,
                actions,
                entry_focused,
                delete_focused,
            ));
        }
    }

    order.finish();
    Container::new(col.finish()).finish()
}

/// One group: its name, how many secrets it holds, and its delete control. The
/// name opens the group; the control beside it removes the group.
fn build_group_row(
    app: &AppContext,
    group: &SecretGroup,
    open: bool,
    actions: &UiActions,
    row_focused: bool,
    delete_focused: bool,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let name = Text::new(group.name.clone())
        .with_theme_color(ColorToken::Text, app)
        .with_font_size(12.0)
        .finish();
    let count = Text::new(format!(
        "{} secret{}",
        group.entries.len(),
        if group.entries.len() == 1 { "" } else { "s" }
    ))
    .with_theme_color(ColorToken::Muted, app)
    .with_font_size(11.0)
    .finish();

    let on_open = actions.on_environment_open_group.clone();
    let id = group.id.clone();
    let row = HoverRow::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(name)
            .with_child(count)
            .with_child(Spacer::new().finish())
            .finish(),
    )
    .with_selected(open)
    .with_padding(EdgeInsets::uniform(sm))
    .with_corner_radius(app.theme.radius_px())
    .with_on_click(move || (on_open.borrow_mut())(id.clone()))
    .finish();

    let delete = icon_button(
        app,
        "trash",
        actions.on_environment_delete_group.clone(),
        group.id.clone(),
    );
    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(focus_ring(app, row, row_focused)).finish())
            .with_child(focus_ring(app, delete, delete_focused))
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, sm, 0.0, sm))
    .finish()
}

/// One secret in the open group: its name, its value masked, and its delete
/// control. Activating the row loads the entry back into the fields to edit.
fn build_secret_row(
    app: &AppContext,
    entry: &SecretEntry,
    actions: &UiActions,
    row_focused: bool,
    delete_focused: bool,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let name = Text::new(entry.name.clone())
        .with_theme_color(ColorToken::Text, app)
        .with_font_size(12.0)
        .finish();
    // The value is never drawn: the pane shows that the entry has one, and an
    // edit loads the real value into the field.
    let value = Text::new("••••••")
        .with_theme_color(ColorToken::Muted, app)
        .with_font_size(12.0)
        .finish();

    let on_edit = actions.on_environment_edit_secret.clone();
    let id = entry.id.clone();
    let row = HoverRow::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(name)
            .with_child(Spacer::new().finish())
            .with_child(value)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(sm))
    .with_corner_radius(app.theme.radius_px())
    .with_on_click(move || (on_edit.borrow_mut())(id.clone()))
    .finish();

    let delete = icon_button(
        app,
        "trash",
        actions.on_environment_delete_secret.clone(),
        entry.id.clone(),
    );
    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(focus_ring(app, row, row_focused)).finish())
            .with_child(focus_ring(app, delete, delete_focused))
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, sm, 0.0, sm))
    .finish()
}

/// A trash control that hands its argument to an action; the sibling of the row
/// it sits beside, never a child of it (a `HoverRow` swallows the click).
fn icon_button(
    app: &AppContext,
    icon: &'static str,
    action: Rc<RefCell<dyn FnMut(String)>>,
    argument: String,
) -> Box<dyn Element> {
    TopbarButton::new(
        Icon::new(icon)
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (action.borrow_mut())(argument.clone()))
    .finish()
}

fn build_models(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

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

    let reload_focused = order.take(SettingsControl::ReloadModels);
    let on_reload = actions.on_reload_model_config.clone();
    let reload = Button::new(Text::new("Reload from config").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_reload.borrow_mut())())
        .finish();

    col = col
        .with_child(Divider::horizontal().finish())
        .with_child(
            Container::new(focus_ring(app, reload, reload_focused))
                .with_padding_uniform(md)
                .finish(),
        );

    order.finish();
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
