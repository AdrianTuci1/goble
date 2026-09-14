use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::render::RenderCommand;

use super::*;

#[test]
fn categories_match_grok_build_labels() {
    assert_eq!(SettingsCategory::ALL.len(), 7);
    let labels: Vec<&str> = SettingsCategory::ALL.iter().map(|c| c.label()).collect();
    assert_eq!(
        labels,
        vec![
            "Appearance",
            "Mouse",
            "Editor & Input",
            "Agent & Approval",
            "Environment",
            "Models",
            "Advanced",
        ]
    );
}

/// A whole app tree with the Settings overlay open, plus its live state.
fn settings_root(
    category: SettingsCategory,
) -> (
    Box<dyn Element>,
    Rc<RefCell<crate::state::UiState>>,
    tempfile::TempDir,
) {
    let (root, state, dir, _desktop) = settings_root_with_desktop(category);
    (root, state, dir)
}

/// The same tree, with the service handle kept: the tests that assert what was
/// persisted (a switch in the store, a group of secrets in the store) need to
/// read the store back rather than the pane's own view of it.
fn settings_root_with_desktop(
    category: SettingsCategory,
) -> (
    Box<dyn Element>,
    Rc<RefCell<crate::state::UiState>>,
    tempfile::TempDir,
    std::sync::Arc<goble_desktop_service::DesktopState>,
) {
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let root = RootView::new(&AppContext::default(), &desktop, None);
    let state = root.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        s.settings_overlay_open = true;
        s.settings_select_category(category);
    }
    (Box::new(root) as Box<dyn Element>, state, dir, desktop)
}

fn key(name: &str) -> DispatchedEvent {
    DispatchedEvent::KeyDown {
        key: name.to_string(),
        modifiers: ModifiersState::none(),
    }
}

/// Dispatch one key through the whole app tree, after a frame, so a handler
/// that captured state when the tree was built sees the last key's effect (and
/// the overlay knows its own origin, which it learns while painting). The
/// running app rebuilds and repaints between events the same way.
fn press(root: &mut Box<dyn Element>, app: &AppContext, name: &str) -> bool {
    let constraint = SizeConstraint::loose(vec2f(1024.0, 768.0));
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let mut ctx = EventContext::default();
    root.dispatch_event(&key(name), &mut ctx, app)
}

#[test]
fn the_arrow_keys_move_the_category_and_escape_closes() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Appearance);

    assert!(press(&mut root, &app, "ArrowDown"));
    assert_eq!(state.borrow().settings_category, SettingsCategory::Mouse);
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_category, SettingsCategory::EditorInput);
    press(&mut root, &app, "ArrowUp");
    assert_eq!(state.borrow().settings_category, SettingsCategory::Mouse);

    // Stepping up from the first category wraps to the last and back.
    state.borrow_mut().settings_category = SettingsCategory::Appearance;
    press(&mut root, &app, "ArrowUp");
    assert_eq!(state.borrow().settings_category, SettingsCategory::Advanced);
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_category, SettingsCategory::Appearance);

    assert!(state.borrow().settings_overlay_open, "the overlay is open");
    assert!(press(&mut root, &app, "Escape"));
    assert!(!state.borrow().settings_overlay_open, "Escape closes it");
}

/// A modified arrow is a pane-navigation shortcut (Ctrl/Cmd+Arrow), not a
/// settings navigation key, so the overlay must let it through.
#[test]
fn a_modified_arrow_does_not_move_the_category() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let mut ctx = EventContext::default();
    root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "ArrowDown".to_string(),
            modifiers: ModifiersState {
                command: true,
                ..Default::default()
            },
        },
        &mut ctx,
        &app,
    );
    assert_eq!(
        state.borrow().settings_category,
        SettingsCategory::Mouse,
        "Cmd+Arrow belongs to pane navigation"
    );
}

/// The overlay is a wide panel inset a few pixels from the window edges, not a
/// small centered card: its background is the viewport minus the inset.
#[test]
fn the_settings_panel_is_the_window_minus_the_inset() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, _state, _dir) = settings_root(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);

    let bg = app.theme.color(goble_ui::theme::ColorToken::Bg);
    let panel = commands.iter().find_map(|c| match c {
        RenderCommand::FillRect { rect, color, .. }
            if *color == bg
                && rect.min_x() == crate::ui::SETTINGS_OVERLAY_INSET
                && rect.min_y() == crate::ui::SETTINGS_OVERLAY_INSET =>
        {
            Some(*rect)
        }
        _ => None,
    });
    let panel = panel.expect("the settings panel paints its own background");
    assert_eq!(panel.width(), window.x - crate::ui::SETTINGS_OVERLAY_INSET * 2.0);
    assert_eq!(panel.height(), window.y - crate::ui::SETTINGS_OVERLAY_INSET * 2.0);
}

/// Lay out and paint one frame, returning its render commands. A frame has to
/// happen before a dispatch: the overlay learns its own origin while painting.
fn frame(root: &mut Box<dyn Element>, app: &AppContext, window: Vector2F) -> Vec<RenderCommand> {
    let constraint = SizeConstraint::loose(window);
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// Appearance draws exactly one color wheel — one hue ring and one
/// saturation/value square — and the square reports into the channel the
/// swatch column selected, not into all three at once.
#[test]
fn the_appearance_pane_has_one_wheel_wired_to_the_selected_channel() {
    use crate::ui::color_picker::SV_ROWS;

    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Appearance);
    state.borrow_mut().theme_color_target = crate::ui::color_picker::ColorTarget::Accent;
    let commands = frame(&mut root, &app, window);

    let rings: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawImage { rect, source, .. } if source.contains("ring") => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rings.len(), 1, "one wheel, not one per theme channel");
    let rows = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRectFadeRight { .. }))
        .count();
    assert_eq!(rows, SV_ROWS, "one saturation/value square");

    // Click the bright, saturated corner of the square: it must land on the
    // selected channel (Accent) and leave the others untouched.
    let ring = rings[0];
    let click = vec2f(
        ring.min_x() + ring.width() * 0.5 + 40.0,
        ring.min_y() + ring.height() * 0.5 - 40.0,
    );
    let mut ctx = EventContext::default();
    assert!(
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: click,
                button: 0
            },
            &mut ctx,
            &app
        ),
        "a click inside the square is handled"
    );
    let state = state.borrow();
    let accent = state.theme_accent.clone().expect("Accent got the new color");
    assert_ne!(accent, "#000000", "the picked color is a real color");
    assert!(state.theme_primary.is_none(), "Primary was not touched");
    assert!(state.theme_secondary.is_none(), "Secondary was not touched");
}

/// The swatch column retargets the one wheel: clicking a channel selects it,
/// and the wheel then edits that channel's stored color.
#[test]
fn clicking_a_swatch_retargets_the_wheel() {
    use crate::ui::color_picker::ColorTarget;

    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Appearance);
    let commands = frame(&mut root, &app, window);
    assert_eq!(
        state.borrow().theme_color_target,
        ColorTarget::Primary,
        "Primary is the default target"
    );

    // The swatch rows are the only place the channel labels are drawn in the
    // wheel block; take the "Accent" one and click on it.
    let accent = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "Accent" => Some(*origin),
            _ => None,
        })
        .expect("the Accent swatch row is drawn");
    let mut ctx = EventContext::default();
    // Buttons fire on release, so the click is a down/up pair on the row.
    let at = accent + vec2f(2.0, 2.0);
    assert!(root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: at,
            button: 0
        },
        &mut ctx,
        &app
    ));
    assert!(
        root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: at,
                button: 0
            },
            &mut ctx,
            &app
        ),
        "a click on the swatch row is handled"
    );
    assert_eq!(state.borrow().theme_color_target, ColorTarget::Accent);
}


// ---- The overlay's two-region keyboard model -------------------------------

/// The rail's arrows still move the category; `Right` moves the keyboard into
/// the pane, where the arrows move between the controls the pane drew, and
/// `Left` walks back out.
#[test]
fn the_arrows_move_the_region_that_holds_the_keyboard() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);

    // The rail, as before: Down/Up step the category.
    assert!(press(&mut root, &app, "ArrowDown"));
    assert_eq!(state.borrow().settings_category, SettingsCategory::EditorInput);
    state.borrow_mut().settings_select_category(SettingsCategory::Mouse);

    // Right moves into the pane, onto its first control.
    assert!(press(&mut root, &app, "ArrowRight"));
    {
        let s = state.borrow();
        assert_eq!(s.settings_focus, SettingsFocus::Pane);
        assert_eq!(s.settings_pane_focus, 0);
        assert_eq!(
            s.settings_focused_control(),
            Some(SettingsControl::InvertScroll)
        );
    }

    // Down steps the pane's own order and leaves the category alone.
    press(&mut root, &app, "ArrowDown");
    {
        let s = state.borrow();
        assert_eq!(s.settings_focused_control(), Some(SettingsControl::ScrollSpeed));
        assert_eq!(
            s.settings_category,
            SettingsCategory::Mouse,
            "the pane's arrows do not move the category"
        );
    }

    // Down at the last control stops there rather than wrapping.
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_pane_focus, 1);
    press(&mut root, &app, "ArrowUp");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::InvertScroll)
    );

    // The switch holds no discrete value, so Left is "back to the rail".
    press(&mut root, &app, "ArrowLeft");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Rail);
    assert!(state.borrow().settings_overlay_open, "the overlay stayed open");

    // Tab goes back in, and a category change resets the pane's focus.
    press(&mut root, &app, "Tab");
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_pane_focus, 1);
    state.borrow_mut().settings_select_category(SettingsCategory::EditorInput);
    assert_eq!(
        state.borrow().settings_pane_focus,
        0,
        "a new category starts the pane's focus at its first control"
    );
}

/// `Enter` on a focused switch flips it, and the flip is persisted: the
/// assertion reads the setting back out of the store, not out of the pane.
#[test]
fn enter_flips_a_switch_and_the_setting_is_persisted() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::AgentApproval);
    assert!(!desktop.get_vim_mode(), "vim mode starts off");

    press(&mut root, &app, "ArrowRight"); // into the pane: Auto-approve
    press(&mut root, &app, "ArrowDown"); // Vim mode
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::VimMode)
    );
    assert!(press(&mut root, &app, "Enter"));
    assert!(state.borrow().vim_mode, "the switch flipped");
    assert!(desktop.get_vim_mode(), "and the flip was persisted");

    // A second Enter flips it back, and that is persisted too.
    press(&mut root, &app, "Enter");
    assert!(!state.borrow().vim_mode);
    assert!(!desktop.get_vim_mode());
}

/// `Left`/`Right` step a control that holds a discrete value (the stepper),
/// and a control that does not is left alone by `Right`.
#[test]
fn left_and_right_adjust_the_focused_stepper() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    press(&mut root, &app, "ArrowRight"); // the switch
    press(&mut root, &app, "ArrowDown"); // the stepper

    let before = state.borrow().settings_scroll_speed;
    assert!(press(&mut root, &app, "ArrowRight"));
    assert_eq!(state.borrow().settings_scroll_speed, before + 1);
    assert!(press(&mut root, &app, "ArrowLeft"));
    assert_eq!(state.borrow().settings_scroll_speed, before);
    assert_eq!(
        state.borrow().settings_focus,
        SettingsFocus::Pane,
        "an adjustable control keeps the keyboard in the pane"
    );
    assert_eq!(state.borrow().settings_category, SettingsCategory::Mouse);

    // The switch above it has no discrete value, so Right does nothing to it
    // (Left there returns to the rail, asserted in the test above).
    press(&mut root, &app, "ArrowUp");
    let clicked = state.borrow().settings_invert_scroll;
    press(&mut root, &app, "ArrowRight");
    assert_eq!(
        state.borrow().settings_invert_scroll,
        clicked,
        "Right does not toggle a switch"
    );
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
}

/// `Escape` steps out one level at a time, and a field the user typed into is
/// reachable and leaveable: it takes the caret on Enter, keeps every printable
/// key (Space included), and gives the caret up on Escape.
#[test]
fn a_pane_text_field_takes_the_caret_and_escape_steps_back_out() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Environment);

    press(&mut root, &app, "ArrowRight");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    assert!(!state.borrow().settings_pane_field_active, "no caret yet");

    // Enter puts the caret in the field, and typing reaches it — including
    // Space, which is the overlay's own key outside a field.
    press(&mut root, &app, "Enter");
    assert!(state.borrow().settings_pane_field_active);
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, " ");
    assert_eq!(state.borrow().settings_environment_group_draft, "prod ");

    // Escape gives the caret up without leaving the field or the pane, then
    // leaves the pane, then closes the overlay.
    press(&mut root, &app, "Escape");
    assert!(!state.borrow().settings_pane_field_active);
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    assert!(state.borrow().settings_overlay_open);

    press(&mut root, &app, "Escape");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Rail);
    assert!(state.borrow().settings_overlay_open);

    press(&mut root, &app, "Escape");
    assert!(!state.borrow().settings_overlay_open, "and then it closes");
}

// ---- Settings -> Environment ----------------------------------------------

/// The Environment pane's whole lifecycle from the keyboard, asserted against
/// the store: a group is created by name, a secret is added to it, the secret
/// is removed, and then the group is deleted with nothing left behind.
#[test]
fn the_environment_pane_creates_adds_and_deletes_through_the_store() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::Environment);

    // Create a group: type its name, then activate "Create group".
    press(&mut root, &app, "Tab"); // into the pane, on the name field
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    press(&mut root, &app, "Enter");
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Escape"); // let the caret go
    press(&mut root, &app, "ArrowDown"); // Create group
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentCreateGroup)
    );
    press(&mut root, &app, "Enter");

    let groups = desktop.environment_groups().expect("read the store back");
    assert_eq!(groups.len(), 1, "a group row was written");
    assert_eq!(groups[0].name, "prod");
    let group_id = groups[0].id.clone();
    assert_eq!(
        state.borrow().settings_environment_open_group.as_deref(),
        Some(group_id.as_str()),
        "the new group is the one the pane shows"
    );

    // The open group adds its own slots: its row, its delete control, the two
    // fields and the save button.
    press(&mut root, &app, "ArrowDown"); // Group(prod)
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroup(group_id.clone()))
    );
    press(&mut root, &app, "ArrowDown"); // DeleteGroup(prod)
    press(&mut root, &app, "ArrowDown"); // the secret-name field
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentSecretName)
    );
    press(&mut root, &app, "Enter");
    for c in ["A", "P", "I", "_", "K", "E", "Y"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Escape");
    press(&mut root, &app, "ArrowDown"); // the secret-value field
    press(&mut root, &app, "Enter");
    for c in ["s", "k", "-", "1"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Escape");
    press(&mut root, &app, "ArrowDown"); // Save secret
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentSaveSecret)
    );
    press(&mut root, &app, "Enter");

    let groups = desktop.environment_groups().expect("read the store back");
    assert_eq!(groups[0].entries.len(), 1, "the secret row was written");
    assert_eq!(groups[0].entries[0].name, "API_KEY");
    assert_eq!(groups[0].entries[0].value, "sk-1");
    let entry_id = groups[0].entries[0].id.clone();
    assert!(
        state.borrow().settings_environment_secret_name.is_empty(),
        "the fields are cleared once the secret is stored"
    );

    // Activating the secret's row loads it back for editing...
    press(&mut root, &app, "Enter"); // (the save button again is a no-op: empty name)
    press(&mut root, &app, "ArrowDown"); // Secret(API_KEY)
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentSecret(entry_id.clone()))
    );
    press(&mut root, &app, "Enter");
    assert_eq!(state.borrow().settings_environment_secret_name, "API_KEY");
    assert_eq!(state.borrow().settings_environment_secret_value, "sk-1");
    assert_eq!(
        state.borrow().settings_environment_editing.as_deref(),
        Some("API_KEY")
    );

    // ...and its own delete control removes just that entry.
    press(&mut root, &app, "ArrowDown");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentDeleteSecret(entry_id))
    );
    press(&mut root, &app, "Enter");
    let groups = desktop.environment_groups().expect("read the store back");
    assert_eq!(groups.len(), 1, "the group is still there");
    assert!(groups[0].entries.is_empty(), "and the secret is gone");

    // The group's own delete control takes the group and leaves nothing. The
    // row the keyboard was on is gone, so the pane's order is the seven slots
    // left: walk back up to the group's delete control.
    press(&mut root, &app, "ArrowUp");
    press(&mut root, &app, "ArrowUp");
    press(&mut root, &app, "ArrowUp");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentDeleteGroup(group_id.clone()))
    );
    press(&mut root, &app, "Enter");
    assert!(
        desktop.environment_groups().expect("read the store back").is_empty(),
        "the group and everything in it is gone"
    );
    assert!(state.borrow().settings_environment_open_group.is_none());
}

/// A click on a group row and on its delete control still do what the keys do:
/// the two paths are one implementation.
#[test]
fn the_environment_rows_answer_the_mouse_as_well_as_the_keys() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::Environment);
    let group = desktop.create_environment_group("staging").expect("create");
    state
        .borrow_mut()
        .refresh_environment_groups(Some(&desktop));
    assert_eq!(
        state.borrow().settings_environment_groups.len(),
        1,
        "the pane was reloaded from the store"
    );
    assert!(state.borrow().settings_environment_open_group.is_none());

    // Click the group's own label: the group opens (the pane shows its
    // secrets, whose fields join the order).
    let commands = frame(&mut root, &app, window);
    let at = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "staging" => Some(*origin),
            _ => None,
        })
        .expect("the group row is drawn");
    let mut ctx = EventContext::default();
    let click = at + vec2f(2.0, 2.0);
    root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: click,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: click,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert_eq!(
        state.borrow().settings_environment_open_group.as_deref(),
        Some(group.id.as_str()),
        "the click opened the group"
    );
}
