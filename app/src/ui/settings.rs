//! Settings: a compact sheet centered over the workspace, a rail of pages
//! beside the content column of the page the rail has selected. The pages are
//! the grok-build categories (Appearance, Mouse, Editor & Input, Agent &
//! Approval, Environment, Models, Advanced), each a flat list of rows: a muted
//! heading per group, one row per setting with the label on the left and the
//! value or the control on the right, and a rule between the rows. There are no
//! cards — the sheet is one surface, like the keyboard-shortcuts panel.
//! Model configuration is done by editing the global `~/.goble/config.toml`;
//! this sheet only lists the resulting models and offers a reload.
//!
//! # Keyboard model
//!
//! The whole sheet is operable from the keyboard. There are **two focus
//! regions** — the rail and the content pane — and the keyboard is in exactly
//! one of them. `UiState::settings_focus` says which,
//! `UiState::settings_pane_focus` says which pane row the keyboard is on, and
//! `UiState::settings_pane_field_active` says whether that row is a text field
//! the caret has been put into. All three live in app state and reach this
//! module through the per-frame [`UiSnapshot`], so the selection survives the
//! element rebuild.
//!
//! `Enter` **activates the focused row**, whatever kind of row it is:
//!
//! | Focused row | `Enter` |
//! | --- | --- |
//! | a toggle | flips it |
//! | a stepper (scroll speed, font size) | advances it one step, as `Right` does |
//! | a single-choice row (Local/Remote, a theme channel) | selects the named choice |
//! | a list row (an environment group, a secret) | opens it |
//! | a text field | puts the caret in it; `Enter` again commits, `Esc` cancels the edit and keeps the sheet open |
//! | a button row (create, save, reload) | runs it |
//!
//! `Esc` steps back one level and only closes the sheet from the rail:
//! out of a field (cancelling that edit), then out of the pane, then closed.
//! The footer under the pane names the keys the focused row answers right now
//! ([`footer_hints`]), so the behaviour is discoverable without a mouse.
//!
//! Modifier-held keys are never the sheet's: `Ctrl`/`Cmd`/`Alt` plus an arrow
//! is the app's pane navigation (`RootView::dispatch_event`), so those fall
//! through untouched, exactly as they did before — and the window-level chords
//! (`⌘K`, `⌘⇧W`, …) are answered by the root view before the tree is
//! dispatched, so they keep working while the sheet is up.
//!
//! | Key | Rail | Pane |
//! | --- | --- | --- |
//! | `Up` / `Down` | move the page, wrapping | move the pane's focused row, clamped at the ends |
//! | `Right` | into the pane, on its first row | adjust the focused row forward if it holds a discrete value; otherwise nothing |
//! | `Left` | nothing (the rail is the leftmost region) | adjust the focused row back if it holds a discrete value; otherwise back to the rail |
//! | `Enter` / `Tab` / `Space` | into the pane | activate the focused row, per the table above |
//! | `Esc` | close the sheet | back to the rail (or cancel a field edit first) |
//!
//! Opening the sheet starts in the rail, and changing the page resets the
//! pane's focus to that page's first row.
//!
//! **Text fields.** A field only *holds the caret* once Enter has put it there;
//! until then it is just the focused row, drawn with the ring. While a field
//! holds the caret the sheet reserves `Enter` (commit) and `Escape` (cancel) —
//! everything else, printable keys, `Space` and `Backspace` included, must
//! reach the field — so a field the user typed into is never a trap.
//!
//! The pane's focus order is [`pane_controls`]: every interactive row the page
//! draws, in the order it draws it. Each page walks that list while it builds
//! its rows through [`FocusOrder`], which asserts the row being drawn is the
//! one the order holds at that slot and that the page left nothing undrawn —
//! so a control cannot be added to a page and silently skipped by the keys.
//!
//! The focused row is marked with [`ColorToken::Focus`], the same blue the
//! focused text field's ring and caret use.

use std::cell::RefCell;
use std::rc::Rc;

use goble_core::store::{SecretEntry, SecretGroup};
use goble_ui::color::ColorU;
use goble_ui::elements::{
    AppContext, Axis, Border, Button, ButtonVariant, ConstrainedBox, Container,
    CrossAxisAlignment, Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex, HoverRow,
    Icon, KeyHandler, MainAxisAlignment, MainAxisSize, Scrollable, ShortcutHint, ShortcutHints,
    Spacer, Switch, Text, TextInput, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::color_picker::{ColorTarget, ColorWheel, COLOR_TARGET_ORDER};
use super::{
    SettingsCategory, SettingsControl, SettingsFocus, UiActions, UiSnapshot, WorkspaceRouting,
};

/// Width of the sheet. A rail plus a content column wide enough for the longest
/// row it draws (the Environment hint, a model name with its mark, the color
/// wheel beside its label), and no wider: a setting's value should sit a glance
/// away from its label rather than at the far edge of a full-width window.
pub const PANEL_WIDTH: f32 = 640.0;

/// Height of the sheet at the default window size. The dialog caps the panel
/// below 90% of the viewport, so a shorter window (or a larger `ui_zoom`, which
/// shrinks the logical viewport) makes the sheet shorter rather than clipping
/// it; whichever is smaller wins and the content scrolls inside it.
pub const PANEL_HEIGHT: f32 = 520.0;

/// Width of the page rail: the widest page label (`Agent & Approval`) plus the
/// row's own padding, so no label wraps.
pub(crate) const NAV_WIDTH: f32 = 148.0;

/// The rule between the rail and the content column: one hairline, counted in
/// the sheet's own arithmetic (`NAV_WIDTH + 1 + paddings + content`).
pub(crate) const RULE_WIDTH: f32 = 1.0;

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

/// Build the whole settings sheet (title bar + page rail + content pane +
/// footer).
pub fn build_settings_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    let on_close = actions.on_settings_close.clone();
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Settings")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(13.0)
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
            .with_size(24.0)
            .with_on_click(move || (on_close.borrow_mut())())
            .finish(),
        )
        .finish();
    let header = Container::new(header)
        .with_padding(EdgeInsets::new(sm, md, sm, md))
        .finish();

    let nav = build_nav(app, state, actions);
    let pane = build_pane(app, state, actions);

    // The rail keeps its own fixed width; the page takes the rest, scrolling
    // inside what the header and the footer leave. The row is sized to the
    // sheet (`MainAxisSize::Max`) so the stretched page is bounded: an
    // unbounded page would lay its rows out at their intrinsic width, and the
    // spacer that holds a value right would collapse to nothing. The sheet's
    // own width is the dialog's (see `PANEL_WIDTH`), so the content column is
    // exactly `PANEL_WIDTH - NAV_WIDTH - rule - the pane's padding`.
    let body = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Start)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            ConstrainedBox::new(nav)
                .with_max_width(NAV_WIDTH)
                .with_min_width(NAV_WIDTH)
                .finish(),
        )
        .with_child(Divider::vertical().with_thickness(RULE_WIDTH).finish())
        .with_child(
            Expanded::new(
                Container::new(
                    Scrollable::new(pane, Axis::Vertical)
                        .with_state(state.settings_scroll.clone())
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(md))
                .finish(),
            )
            .finish(),
        );

    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(header)
        .with_child(Divider::horizontal().finish())
        .with_child(Expanded::new(body.finish()).finish())
        .with_child(Divider::horizontal().finish())
        .with_child(build_footer(app, state));

    let panel = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_border(Border::all(RULE_WIDTH).with_border_color(app.theme.color(ColorToken::Border)))
        .finish();
    let panel = ConstrainedBox::new(panel)
        .with_max_height(PANEL_HEIGHT)
        .finish();

    // The sheet sees every key before its children do. Modified keys are the
    // app's (pane navigation above all), so they are never the sheet's, and the
    // window-level chords are answered by `RootView::dispatch_event` before the
    // tree is dispatched at all. Which of the remaining keys the sheet keeps
    // depends on where the keyboard is — see the module comment; the short of
    // it is that a field holding the caret keeps `Enter` (commit) and `Escape`
    // (cancel) and lets everything else through to the field.
    let on_step = actions.on_settings_focus_move.clone();
    let on_into_pane = actions.on_settings_focus_into_pane.clone();
    let on_out_of_pane = actions.on_settings_focus_out_of_pane.clone();
    let on_activate = actions.on_settings_activate.clone();
    let on_adjust = actions.on_settings_adjust.clone();
    let on_commit_field = actions.on_settings_commit_field.clone();
    let on_cancel_field = actions.on_settings_cancel_field.clone();
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
            // Only `Enter` and `Escape` are the sheet's while a field holds the
            // caret; letters, `Space` and `Backspace` go to the field.
            match key {
                "Enter" => (on_commit_field.borrow_mut())(),
                "Escape" => (on_cancel_field.borrow_mut())(),
                _ => return false,
            }
            return true;
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
            // `Left` walks back out of the pane unless the focused row has a
            // discrete value of its own to walk back through.
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

/// The sheet's footer: the keys the focused region answers right now, drawn as
/// the composer's own key caps. It is the only place the sheet's key map is
/// written down for the reader, so it is built from [`footer_hints`] — the same
/// list the tests read.
fn build_footer(app: &AppContext, state: &UiSnapshot) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let control = pane_controls(
        state.settings_category,
        &state.settings_environment_groups,
        state.settings_environment_open_group.as_deref(),
    )
    .get(state.settings_pane_focus)
    .cloned();
    let hints = footer_hints(
        state.settings_focus,
        control.as_ref(),
        state.settings_pane_field_active,
    );
    Container::new(ShortcutHints::new(hints).finish(app))
        .with_padding(EdgeInsets::new(sm, md, sm, md))
        .finish()
}

/// What the footer says right now: the keys the focused region answers.
///
/// One list, read by the drawn footer and by the tests, so what the sheet
/// promises on screen is what the key handler does.
pub(crate) fn footer_hints(
    focus: SettingsFocus,
    control: Option<&SettingsControl>,
    field_active: bool,
) -> Vec<ShortcutHint> {
    match focus {
        SettingsFocus::Rail => vec![
            ShortcutHint::new(&["↑", "↓"], "page"),
            ShortcutHint::new(&["↵"], "open the page"),
            ShortcutHint::new(&["Esc"], "close"),
        ],
        SettingsFocus::Pane => {
            // A field with the caret is the one row whose keys are not the
            // sheet's own: the footer says what the two reserved keys do.
            if field_active {
                return vec![
                    ShortcutHint::new(&["↵"], "commit"),
                    ShortcutHint::new(&["Esc"], "cancel the edit"),
                ];
            }
            let mut hints = vec![ShortcutHint::new(&["↑", "↓"], "row")];
            match control {
                Some(control) if control.is_text_field() => {
                    hints.push(ShortcutHint::new(&["↵"], "edit"));
                }
                Some(SettingsControl::ColorWheel) => {}
                Some(SettingsControl::Route(_)) => {
                    hints.push(ShortcutHint::new(&["←", "→"], "choose"));
                    hints.push(ShortcutHint::new(&["↵"], "select"));
                }
                Some(control) if control.has_discrete_values() => {
                    hints.push(ShortcutHint::new(&["←", "→"], "change"));
                    hints.push(ShortcutHint::new(&["↵"], "increase"));
                }
                Some(control) if is_toggle(control) => {
                    hints.push(ShortcutHint::new(&["↵"], "toggle"));
                }
                Some(SettingsControl::EnvironmentGroup(_)) => {
                    hints.push(ShortcutHint::new(&["↵"], "open"));
                }
                // The remaining rows are buttons and fields/entries: Enter
                // runs the one and opens the other.
                Some(_) => hints.push(ShortcutHint::new(&["↵"], "run")),
                None => {}
            }
            hints.push(ShortcutHint::new(&["Esc"], "back"));
            hints
        }
    }
}

/// Whether the row is a two-state toggle, which `Enter` flips.
fn is_toggle(control: &SettingsControl) -> bool {
    matches!(
        control,
        SettingsControl::DarkMode
            | SettingsControl::InvertScroll
            | SettingsControl::AutoApprove
            | SettingsControl::VimMode
    )
}

/// Left column listing each settings page; the page being shown is marked, and
/// while the rail holds the keyboard it wears the focus ring. Its rows are flat,
/// like every other list in the app: a label, a rule, the next label.
fn build_nav(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min);

    for (index, &cat) in SettingsCategory::ALL.iter().enumerate() {
        if index > 0 {
            col = col.with_child(Divider::horizontal().finish());
        }
        let on_select = actions.on_settings_category.clone();
        let selected = cat == state.settings_category;
        let label = Text::new(cat.label())
            .with_theme_color(
                if selected { ColorToken::Text } else { ColorToken::Muted },
                app,
            )
            .with_font_size(12.0)
            .with_max_lines(1)
            .finish();
        let row = HoverRow::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(label)
                .finish(),
        )
        .with_selected(selected)
        .with_padding(EdgeInsets::uniform(sm))
        .with_on_click(move || (on_select.borrow_mut())(cat))
        .finish();
        let focused = selected && state.settings_focus == SettingsFocus::Rail;
        col = col.with_child(focus_ring(app, row, focused));
    }

    Container::new(col.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(sm))
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

/// A group heading inside a page: the muted caption the app's own lists use.
fn heading(app: &AppContext, title: &str) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    Container::new(
        Text::new(title)
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(11.0)
            .with_max_lines(1)
            .finish(),
    )
    .with_padding(EdgeInsets::new(sm, md, sm, xs))
    .finish()
}

/// The rule between two rows of a page. A page is a flat list: no cards, a rule
/// per row, exactly like the keyboard-shortcuts panel.
fn rule() -> Box<dyn Element> {
    Divider::horizontal().finish()
}

/// One setting: the label on the left, the control or the value on the right,
/// and the ring when the keyboard is on the row. `right` is the control the
/// mouse already uses, so a click and `Enter` do the same thing.
fn setting_row(
    app: &AppContext,
    label: &str,
    right: Box<dyn Element>,
    focused: bool,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(right)
        .finish();
    Container::new(focus_ring(app, row, focused))
        .with_padding(EdgeInsets::uniform(sm))
        .finish()
}

/// The same row for a control that fills the space right of its label — a text
/// field, which is only worth typing into if it has the width.
fn field_row(
    app: &AppContext,
    label: &str,
    field: Box<dyn Element>,
    focused: bool,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(Expanded::new(field).finish())
        .finish();
    Container::new(focus_ring(app, row, focused))
        .with_padding(EdgeInsets::uniform(sm))
        .finish()
}

/// A row that runs something: its button sits on the right, where a setting's
/// value sits, so the page reads as one column of labels and one of controls.
fn action_row(app: &AppContext, button: Box<dyn Element>, focused: bool) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Spacer::new().finish())
            .with_child(focus_ring(app, button, focused))
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(sm))
    .finish()
}

/// The `- value +` stepper a row draws on its right. The two buttons are the
/// mouse's path to the same value `Left`/`Right`/`Enter` step from the keyboard.
fn stepper(
    app: &AppContext,
    value: String,
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
    Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Xs))
        .with_child(minus)
        .with_child(
            Text::new(value)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(plus)
        .finish()
}

/// A two-state row: the label on the left, the `Switch` on the right.
fn switch_row(
    app: &AppContext,
    label: &str,
    checked: bool,
    on_change: Rc<RefCell<dyn FnMut(bool)>>,
    focused: bool,
) -> Box<dyn Element> {
    let control = Switch::new()
        .with_checked(checked)
        .with_on_change(move |v| (on_change.borrow_mut())(v))
        .finish();
    setting_row(app, label, control, focused)
}

fn build_appearance(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(heading(app, "Theme"));
    // Rows are drawn in the focus order, so the ring walks down the page with
    // the arrow keys.
    col = col.with_child(switch_row(
        app,
        "Dark mode",
        state.settings_dark_mode,
        actions.on_toggle_dark_mode.clone(),
        order.take(SettingsControl::DarkMode),
    ));
    col = col.with_child(rule());
    col = col.with_child(heading(app, "Colors"));
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
        // The row itself is the hit target, so the swatch beside its label is
        // clickable exactly where the label is.
        let row = HoverRow::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Text::new(target.label())
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(Spacer::new().finish())
                .with_child(swatch)
                .finish(),
        )
        .with_selected(selected)
        .with_padding(EdgeInsets::uniform(sm))
        .with_on_click(move || (on_select.borrow_mut())(target))
        .finish();
        col = col.with_child(focus_ring(app, row, focused));
        col = col.with_child(rule());
    }

    // The wheel edits one channel at a time: the rows above pick which, and
    // this row is the wheel itself with the channel it is editing beside it.
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
    let wheel_row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_main_axis_size(MainAxisSize::Min)
                .with_child(
                    Text::new("Custom color")
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(
                    Text::new(format!("{}  {}", target.label(), color.to_hex_string()))
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .with_max_lines(1)
                        .finish(),
                )
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(focus_ring(
            app,
            wheel,
            order.take(SettingsControl::ColorWheel),
        ))
        .finish();
    col = col.with_child(
        Container::new(wheel_row)
            .with_padding(EdgeInsets::new(sm, 0.0, sm, sm))
            .finish(),
    );
    order.finish();
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
    let speed = stepper(
        app,
        state.settings_scroll_speed.to_string(),
        on_dec,
        on_inc,
    );
    let rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(heading(app, "Scrolling"))
        .with_child(switch_row(
            app,
            "Invert scroll",
            state.settings_invert_scroll,
            actions.on_toggle_invert_scroll.clone(),
            order.take(SettingsControl::InvertScroll),
        ))
        .with_child(rule())
        .with_child(setting_row(
            app,
            "Scroll speed",
            speed,
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
    let size = stepper(app, format!("{:.1}x", state.settings_font_size), on_dec, on_inc);
    let rows = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(heading(app, "Text"))
        .with_child(setting_row(
            app,
            "Font size",
            size,
            order.take(SettingsControl::FontSize),
        ))
        .with_child(rule())
        .with_child(
            Container::new(
                Text::new("Zoom applies to the whole window: Cmd/Ctrl+Plus and Minus do the same.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Sm)))
            .finish(),
        )
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
        .with_child(heading(app, "Behaviour"))
        .with_child(switch_row(
            app,
            "Auto-approve",
            state.auto_approve,
            on_auto_approve,
            auto_approve_focused,
        ))
        .with_child(rule())
        .with_child(switch_row(
            app,
            "Vim mode",
            state.vim_mode,
            on_vim,
            vim_focused,
        ))
        .with_child(rule())
        .with_child(heading(app, "Where the agent runs"))
        .with_child(
            Container::new(routing)
                .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Sm)))
                .finish(),
        )
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
        .with_child(heading(app, "Groups of secrets"))
        .with_child(
            Container::new(
                Text::new("Stored in the local database, unencrypted, for a turn that runs remotely.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(sm, 0.0, sm, app.theme.spacing_px(SpacingToken::Xs)))
            .finish(),
        );

    // New group: a name field and the button that creates it, on one row.
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
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(
                    Text::new("New group")
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(12.0)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(Expanded::new(focus_ring(app, group_field, group_field_focused)).finish())
                .with_child(focus_ring(app, create, create_focused))
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(sm))
        .finish(),
    );

    if groups.is_empty() {
        col = col.with_child(rule());
        col = col.with_child(
            Container::new(
                Text::new("No groups yet. Name one above to start.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );
    }
    for group in groups {
        col = col.with_child(rule());
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
        col = col.with_child(rule());
        col = col.with_child(heading(app, &format!("Secrets in {}", group.name)));

        let name_focused = order.take(SettingsControl::EnvironmentSecretName);
        let on_name = actions.on_environment_secret_name_change.clone();
        let name_field = TextInput::new()
            .with_value(state.settings_environment_secret_name.clone())
            .with_placeholder("Secret name, e.g. API_KEY")
            .with_focused(name_focused && state.settings_pane_field_active)
            .with_on_change(move |value| (on_name.borrow_mut())(value))
            .finish();
        col = col.with_child(field_row(app, "Name", name_field, name_focused));

        let value_focused = order.take(SettingsControl::EnvironmentSecretValue);
        let on_value = actions.on_environment_secret_value_change.clone();
        let value_field = TextInput::new()
            .with_value(state.settings_environment_secret_value.clone())
            .with_placeholder("Secret value")
            .with_focused(value_focused && state.settings_pane_field_active)
            .with_on_change(move |value| (on_value.borrow_mut())(value))
            .finish();
        col = col.with_child(field_row(app, "Value", value_field, value_focused));

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
        col = col.with_child(action_row(app, save, save_focused));

        for entry in &group.entries {
            col = col.with_child(rule());
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
        if group.entries.is_empty() {
            col = col.with_child(rule());
            col = col.with_child(
                Container::new(
                    Text::new("No secrets in this group yet.")
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(sm))
                .finish(),
            );
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
    // The group's name on the left, what it holds on the right, exactly like
    // the other rows of the sheet; its trash control sits beside the row.
    let row = HoverRow::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(name)
            .with_child(Spacer::new().finish())
            .with_child(count)
            .finish(),
    )
    .with_selected(open)
    .with_padding(EdgeInsets::uniform(sm))
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
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(name)
            .with_child(Spacer::new().finish())
            .with_child(value)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(sm))
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
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let controls = order_for(state);
    let mut order = FocusOrder::new(state, &controls);

    let mut col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(heading(app, "Models from ~/.goble/config.toml"));
    if state.models.is_empty() {
        col = col.with_child(
            Container::new(
                Text::new("No models configured yet. Add one to the config file and reload.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        );
    }
    // The list is read-only here: which model a conversation uses is the
    // composer's choice, so these rows are not focus stops.
    for (index, model) in state.models.iter().enumerate() {
        if index > 0 {
            col = col.with_child(rule());
        }
        let selected = *model == state.selected_model;
        col = col.with_child(setting_row(
            app,
            model,
            Text::new(if selected { "current" } else { "" })
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish(),
            false,
        ));
    }

    let reload_focused = order.take(SettingsControl::ReloadModels);
    let on_reload = actions.on_reload_model_config.clone();
    let reload = Button::new(Text::new("Reload from config").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_reload.borrow_mut())())
        .finish();

    col = col.with_child(rule());
    col = col.with_child(action_row(app, reload, reload_focused));

    order.finish();
    Container::new(col.finish()).finish()
}

fn build_advanced(app: &AppContext, _state: &UiSnapshot, _actions: &UiActions) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let ssh = ssh_configured();

    let col = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(heading(app, "Machine"))
        .with_child(setting_row(
            app,
            "SSH keys",
            Text::new(if ssh { "configured" } else { "none found" })
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish(),
            false,
        ))
        .with_child(rule())
        .with_child(setting_row(
            app,
            "Config file",
            Text::new("~/.goble/config.toml")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish(),
            false,
        ))
        .with_child(rule())
        .with_child(
            Container::new(
                Text::new("This page only reports: the keys live in ~/.ssh and the config in ~/.goble/config.toml. Nothing here is a control, so the pane takes no keyboard focus.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .with_padding(EdgeInsets::uniform(sm))
            .finish(),
        )
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
