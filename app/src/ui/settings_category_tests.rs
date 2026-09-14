use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{vec2f, RectF, Vector2F};
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

/// The same key with modifiers held, for the window-level chords.
fn chord(name: &str, modifiers: ModifiersState) -> DispatchedEvent {
    DispatchedEvent::KeyDown {
        key: name.to_string(),
        modifiers,
    }
}

/// Dispatch one key through the whole app tree, after a frame, so a handler
/// that captured state when the tree was built sees the last key's effect (and
/// the overlay knows its own origin, which it learns while painting). The
/// running app rebuilds and repaints between events the same way.
fn press(root: &mut Box<dyn Element>, app: &AppContext, name: &str) -> bool {
    press_event(root, app, &key(name))
}

/// The same round trip for a key carrying modifiers.
fn press_event(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    event: &DispatchedEvent,
) -> bool {
    let constraint = SizeConstraint::loose(vec2f(1024.0, 768.0));
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let mut ctx = EventContext::default();
    root.dispatch_event(event, &mut ctx, app)
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

/// The sheet is compact, not a window-sized surface: `PANEL_WIDTH` ×
/// `PANEL_HEIGHT` over the middle of the window, smaller in a shorter window
/// (the dialog caps the panel below 90% of the viewport) rather than clipped by
/// it. Widths are arithmetic: the rail's own fixed width, the rule beside it,
/// the pane's padding, and the scroll viewport, which takes the height left
/// between the header and the footer — 640 = 148 rail + 1 rule + 2×12 padding +
/// 467 of content for the width, and a 406.8 px viewport inside the 520 px
/// panel for the height. Checked at the 1280×800 default, at 1024×768, at the
/// 640×400 logical viewport the largest zoom (2.0) leaves, and in a narrow
/// 560×400 window, where the sheet is clamped to the window's own width.
#[test]
fn the_sheet_is_a_compact_sheet_whose_width_adds_up() {
    use goble_ui::theme::SpacingToken;

    let app = AppContext::default();
    let md = app.theme.spacing_px(SpacingToken::Md);
    let (mut root, _state, _dir) = settings_root(SettingsCategory::Mouse);
    // The header and footer bands, remembered from the first window and held
    // fixed for the rest.
    let mut bands: Option<(f32, f32)> = None;

    for window in [
        vec2f(1280.0, 800.0),
        vec2f(1024.0, 768.0),
        vec2f(640.0, 400.0),
        vec2f(560.0, 400.0),
    ] {
        let commands = frame(&mut root, &app, window);
        let width = crate::ui::settings::PANEL_WIDTH.min(window.x);
        // The dialog caps the panel at 90% of the viewport.
        let height = crate::ui::settings::PANEL_HEIGHT.min(window.y * 0.9);
        // The sheet's own surface, at exactly those bounds and centered: the
        // app's root surface spans the window, so this fill is the sheet's.
        let panel = bg_fills(&commands, &app)
            .into_iter()
            .find(|rect| {
                rect.width() == width
                    && rect.height() == height
                    && rect.min_x() == (window.x - width) * 0.5
                    && rect.min_y() == (window.y - height) * 0.5
            })
            .unwrap_or_else(|| {
                panic!(
                    "no sheet of {width}x{height} centered in {window:?}: {:?}",
                    bg_fills(&commands, &app)
                )
            });

        // The rail draws at its own fixed width, hard against the sheet's left
        // edge. In a narrow window the sheet reaches the window's left edge, so
        // the rail is told apart from the workspace surface under it by the
        // sheet's own band of height.
        let nav = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(goble_ui::theme::ColorToken::Surface)
                        && rect.min_x() == panel.min_x()
                        && rect.min_y() >= panel.min_y()
                        && rect.max_y() <= panel.max_y() =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the rail paints its own background");
        assert_eq!(nav.width(), crate::ui::settings::NAV_WIDTH);

        // The page scrolls in a clipped viewport: nav + rule + the pane's
        // padding + the viewport is the sheet's width, and the viewport's band
        // sits below the header and above the footer.
        let clip = commands
            .iter()
            .rev()
            .find_map(|c| match c {
                RenderCommand::ClipRect(rect)
                    if rect.min_x() >= panel.min_x() && rect.max_x() <= panel.max_x() =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no clip inside {panel:?}"));
        assert_eq!(
            clip.min_x(),
            panel.min_x() + crate::ui::settings::NAV_WIDTH + crate::ui::settings::RULE_WIDTH + md,
            "the viewport starts after the rail, the rule and the pane's padding"
        );
        assert_eq!(
            clip.width(),
            width - crate::ui::settings::NAV_WIDTH - crate::ui::settings::RULE_WIDTH - md * 2.0,
            "nav + rule + paddings + viewport is the sheet's width"
        );
        // The viewport takes exactly the height the two fixed bands leave, and
        // those bands are the sheet's own furniture (the header's padding and
        // title row, the footer's hint line, each closed by a rule): they do
        // not shrink when the sheet is capped in a short window.
        let header_band = clip.min_y() - panel.min_y();
        let footer_band = panel.max_y() - clip.max_y();
        assert!(
            header_band > 0.0 && footer_band > 0.0 && header_band + footer_band < height,
            "the header and the footer leave the viewport a positive height: \
             {header_band} + {footer_band} in {height}"
        );
        match bands {
            None => bands = Some((header_band, footer_band)),
            Some(seen) => assert_eq!(
                (header_band, footer_band),
                seen,
                "the header and footer bands are fixed while the sheet resizes"
            ),
        }
        assert_eq!(
            clip.height(),
            height - header_band - footer_band,
            "the viewport takes the height the header and the footer leave"
        );
        // Both fixed bands are drawn: the header above the viewport and the
        // footer's hints below it.
        let header = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::DrawText { origin, text, .. } if text == "Settings" => Some(*origin),
                _ => None,
            })
            .expect("the sheet draws its title");
        assert!(
            header.y < clip.min_y(),
            "the header is above the viewport: {header:?}"
        );
        let footer = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { origin, .. } if origin.y > clip.max_y() => Some(*origin),
                _ => None,
            })
            .count();
        assert!(footer > 0, "the footer is drawn below the viewport");
    }
}

/// The sheet's own keys take precedence over the workspace while it is open,
/// and the window-level chords are the window's: `⌘K` and `⌘⇧W` still answer,
/// a plain `Enter` reaches the sheet and nothing under it.
#[test]
fn the_window_chords_still_answer_while_the_sheet_is_open() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);

    // Enter is the sheet's: it moves the keyboard into the pane rather than
    // reaching the composer underneath.
    let before = state.borrow().settings_pane_focus;
    assert!(press(&mut root, &app, "Enter"), "the sheet consumes Enter");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
    assert_eq!(state.borrow().settings_pane_focus, before);
    assert!(!state.borrow().command_palette_open);

    let command = ModifiersState {
        command: true,
        ..Default::default()
    };
    assert!(press_event(&mut root, &app, &chord("k", command)));
    assert!(
        state.borrow().command_palette_open,
        "⌘K opens the palette over the sheet"
    );
    press_event(&mut root, &app, &chord("k", command));

    let shift_command = ModifiersState {
        command: true,
        shift: true,
        ..Default::default()
    };
    assert!(press_event(&mut root, &app, &chord("w", shift_command)));
    assert!(
        state.borrow().task_workflow_open,
        "⌘⇧W opens the tasks & workflows overlay while the sheet is up"
    );
    assert!(state.borrow().settings_overlay_open, "and the sheet stays open");
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
/// key (Space included), cancels on Escape, and commits on a second Enter.
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
    // Space, which is the sheet's own key outside a field.
    press(&mut root, &app, "Enter");
    assert!(state.borrow().settings_pane_field_active);
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, " ");
    assert_eq!(state.borrow().settings_environment_group_draft, "prod ");

    // Escape cancels the edit: what the field held when the caret went in is
    // put back, the caret goes, and the sheet stays open on the same row.
    press(&mut root, &app, "Escape");
    assert!(!state.borrow().settings_pane_field_active);
    assert!(
        state.borrow().settings_environment_group_draft.is_empty(),
        "a cancelled edit leaves nothing behind"
    );
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    assert!(state.borrow().settings_overlay_open);

    // Enter again edits, and a second Enter commits: what was typed stays.
    press(&mut root, &app, "Enter");
    assert!(state.borrow().settings_pane_field_active, "the caret is back");
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Enter");
    assert!(!state.borrow().settings_pane_field_active, "committed");
    assert_eq!(state.borrow().settings_environment_group_draft, "prod");
    assert_eq!(
        state.borrow().settings_focus,
        SettingsFocus::Pane,
        "committing keeps the keyboard on the row"
    );
    assert!(state.borrow().settings_overlay_open);

    // Then Escape steps back out: the pane, the rail, and the sheet.
    press(&mut root, &app, "Escape");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Rail);
    assert!(state.borrow().settings_overlay_open);

    press(&mut root, &app, "Escape");
    assert!(!state.borrow().settings_overlay_open, "and then it closes");
}

/// `Enter` is what a row does: a stepper advances one step (the same step
/// `Right` takes), and a single-choice row selects the choice it names.
#[test]
fn enter_advances_a_stepper_and_selects_a_choice() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    press(&mut root, &app, "ArrowRight"); // into the pane: Invert scroll
    press(&mut root, &app, "ArrowDown"); // Scroll speed
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::ScrollSpeed)
    );
    let before = state.borrow().settings_scroll_speed;
    assert!(press(&mut root, &app, "Enter"));
    assert_eq!(state.borrow().settings_scroll_speed, before + 1);
    assert!(
        !state.borrow().settings_invert_scroll,
        "Enter did not reach the switch above the stepper"
    );

    // The font-size stepper is the other one: a tenth per press.
    let (mut root, state, _dir) = settings_root(SettingsCategory::EditorInput);
    press(&mut root, &app, "ArrowRight");
    let before = state.borrow().settings_font_size;
    press(&mut root, &app, "Enter");
    assert!((state.borrow().settings_font_size - (before + 0.1)).abs() < 1e-5);

    // A routing row selects the choice it names.
    let (mut root, state, _dir) = settings_root(SettingsCategory::AgentApproval);
    press(&mut root, &app, "ArrowRight"); // Auto-approve
    press(&mut root, &app, "ArrowDown"); // Vim mode
    press(&mut root, &app, "ArrowDown"); // Local
    press(&mut root, &app, "ArrowDown"); // Remote
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::Route(WorkspaceRouting::Remote))
    );
    press(&mut root, &app, "Enter");
    assert_eq!(
        state.borrow().workspace_routing,
        Some(WorkspaceRouting::Remote)
    );
}

/// The footer is the sheet's key map, drawn: each zone and each kind of row
/// answers with its own words, and the drawn line is that list.
#[test]
fn the_footer_says_what_the_focused_row_answers() {
    use crate::ui::settings::footer_hints;

    let words = |hints: &[goble_ui::elements::ShortcutHint]| -> Vec<String> {
        hints.iter().map(|hint| hint.label().to_string()).collect()
    };
    let rail = footer_hints(SettingsFocus::Rail, None, false);
    assert_eq!(words(&rail), vec!["page", "open the page", "close"]);

    let toggle = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::DarkMode),
        false,
    );
    assert_eq!(words(&toggle), vec!["row", "toggle", "back"]);

    let stepper = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::ScrollSpeed),
        false,
    );
    assert_eq!(words(&stepper), vec!["row", "change", "increase", "back"]);

    let choice = footer_hints(SettingsFocus::Pane, Some(&SettingsControl::Route(WorkspaceRouting::Local)), false);
    assert_eq!(words(&choice), vec!["row", "choose", "select", "back"]);

    let field = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::EnvironmentGroupName),
        false,
    );
    assert_eq!(words(&field), vec!["row", "edit", "back"]);

    // While the caret is in a field only the two reserved keys are the sheet's.
    let editing = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::EnvironmentGroupName),
        true,
    );
    assert_eq!(words(&editing), vec!["commit", "cancel the edit"]);

    let list = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::EnvironmentGroup("g".to_string())),
        false,
    );
    assert_eq!(words(&list), vec!["row", "open", "back"]);

    let wheel = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::ColorWheel),
        false,
    );
    assert_eq!(words(&wheel), vec!["row", "back"]);

    // And the drawn footer follows the keyboard: the rail's words while the
    // rail holds it, the switch's once the pane does.
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);
    for label in words(&rail) {
        assert!(drawn(&commands, &label), "the footer says {label:?}");
    }
    assert!(drawn(&commands, "Esc"), "the Esc cap is drawn");
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "key-arrow-up")),
        "the move keys are drawn as the strip's own caps"
    );

    assert!(press(&mut root, &app, "ArrowRight"));
    state.borrow_mut().settings_pane_focus = 0;
    let commands = frame(&mut root, &app, window);
    assert!(drawn(&commands, "toggle"), "the switch's own verb");
    assert!(
        !drawn(&commands, "open the page"),
        "the rail's words are gone once the pane has the keyboard"
    );
}

/// A page's focus order is the order it draws its rows in: stepping the pane's
/// slot walks the ring down the sheet, or across when two rows share a line
/// (the routing buttons sit side by side), never back up. (The pane's own
/// `FocusOrder` asserts each row is the one the order holds where it is drawn;
/// this asserts the reader's view of it.)
#[test]
fn the_focus_order_walks_down_the_rows_the_page_draws() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    for category in [
        SettingsCategory::Appearance,
        SettingsCategory::Mouse,
        SettingsCategory::EditorInput,
        SettingsCategory::AgentApproval,
        SettingsCategory::Models,
    ] {
        let (mut root, state, _dir) = settings_root(category);
        let controls = state.borrow().settings_pane_controls();
        assert!(!controls.is_empty(), "{category:?} has a row to focus");
        let mut previous: Option<(f32, f32)> = None;
        for slot in 0..controls.len() {
            {
                let mut s = state.borrow_mut();
                s.settings_focus = SettingsFocus::Pane;
                s.settings_pane_focus = slot;
            }
            let commands = frame(&mut root, &app, window);
            let ring = focused_row_ring(&commands, &app, window);
            if let Some((previous_y, previous_x)) = previous {
                assert!(
                    ring.min_y() > previous_y
                        || (ring.min_y() == previous_y && ring.min_x() > previous_x),
                    "{category:?}: {:?} is drawn after the row before it",
                    controls[slot]
                );
            }
            previous = Some((ring.min_y(), ring.min_x()));
        }
    }
}

/// A page taller than the viewport still scrolls inside the sheet: the wheel
/// over the pane moves the content under a viewport that does not move.
#[test]
fn a_long_page_scrolls_inside_the_sheet() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir, _desktop) = settings_root_with_desktop(SettingsCategory::Mouse);
    frame(&mut root, &app, window);
    assert_eq!(
        state.borrow().settings_scroll.borrow().max_offset(),
        0.0,
        "a short page has nothing to scroll"
    );

    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::Environment);
    // Enough groups that the page outruns the ~413 px the header and the footer
    // leave of the sheet at this window.
    let names = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi",
    ];
    for name in names {
        desktop.create_environment_group(name).expect("create group");
    }
    state.borrow_mut().refresh_environment_groups(Some(&desktop));
    assert_eq!(
        state.borrow().settings_environment_groups.len(),
        names.len()
    );

    // The pointer sits over the page's viewport for the frame that precedes the
    // wheel: the region remembers where the pointer was while painting.
    let at = vec2f(
            (window.x - crate::ui::settings::PANEL_WIDTH) * 0.5
                + crate::ui::settings::NAV_WIDTH
                + 40.0,
        300.0,
    );
    let commands = frame_at(&mut root, &app, window, at);
    let before = text_y(&commands, "New group").expect("the page draws its rows");
    let max = state.borrow().settings_scroll.borrow().max_offset();
    assert!(max > 0.0, "the page is taller than the viewport");
    let clip = clip_in_panel(&commands, &app, window);
    assert!(clip.max_y() <= window.y, "the viewport is inside the window");

    let mut ctx = EventContext::default();
    assert!(
        root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -30.0),
            },
            &mut ctx,
            &app
        ),
        "the wheel over the page is consumed"
    );
    assert_eq!(state.borrow().settings_scroll.borrow().offset(), 30.0);

    // The page moved; the viewport did not.
    let commands = frame_at(&mut root, &app, window, at);
    let after = text_y(&commands, "New group").expect("its rows are drawn, clipped or not");
    assert!(
        (before - after - 30.0).abs() < 0.5,
        "the page scrolled by the wheel's delta: {before} -> {after}"
    );
    let scrolled_clip = clip_in_panel(&commands, &app, window);
    assert_eq!(scrolled_clip, clip, "the viewport itself did not move");
}

/// The mouse keeps its paths: the rail's rows select a page, a switch flips, a
/// stepper's `+` steps, and the backdrop closes the sheet.
#[test]
fn the_sheet_still_answers_the_mouse() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);

    // A rail row: the label is inside the row's hit target.
    let models = pane_text(&commands, "Models", window).expect("the rail draws its pages");
    click(&mut root, &app, models + vec2f(2.0, 2.0));
    assert_eq!(state.borrow().settings_category, SettingsCategory::Models);

    // A switch: the 44x24 track in the content pane.
    state.borrow_mut().settings_select_category(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);
    let switch = switch_track(&commands, window).expect("the pane draws its switch");
    click(&mut root, &app, switch.center().to_vector());
    assert!(state.borrow().settings_invert_scroll, "the switch flipped");

    // A stepper: its `+`.
    let plus = pane_text(&commands, "+", window).expect("the stepper draws its buttons");
    let before = state.borrow().settings_scroll_speed;
    click(&mut root, &app, plus + vec2f(2.0, 2.0));
    assert_eq!(state.borrow().settings_scroll_speed, before + 1);

    // The backdrop, outside the sheet.
    click(&mut root, &app, vec2f(8.0, 400.0));
    assert!(!state.borrow().settings_overlay_open, "the backdrop closes it");
}

/// One click: the down/up pair a `Button` and a `HoverRow` both complete on.
fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
    for event in [
        DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        },
    ] {
        press_event(root, app, &event);
    }
}

/// Whether a run of text is painted.
fn drawn(commands: &[RenderCommand], text: &str) -> bool {
    commands.iter().any(|c| matches!(c, RenderCommand::DrawText { text: run, .. } if run == text))
}

/// The origin of a run of text.
fn drawn_text(commands: &[RenderCommand], text: &str) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText { origin, text: run, .. } if run == text => Some(*origin),
        _ => None,
    })
}

/// The origin of a run of text drawn inside the sheet, where a label on the
/// page and a label on the rail cannot be confused with the workspace's own.
fn pane_text(commands: &[RenderCommand], text: &str, window: Vector2F) -> Option<Vector2F> {
    let panel_left = (window.x - crate::ui::settings::PANEL_WIDTH) * 0.5;
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText { origin, text: run, .. }
            if run == text && origin.x >= panel_left =>
        {
            Some(*origin)
        }
        _ => None,
    })
}

/// The y of a run of text.
fn text_y(commands: &[RenderCommand], text: &str) -> Option<f32> {
    drawn_text(commands, text).map(|origin| origin.y)
}

/// The focused row's ring: the sheet's only `ColorToken::Focus` stroke in its
/// content column, painted last because the sheet is the top-most surface.
fn focused_row_ring(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> RectF {
    let focus = app.theme.color(goble_ui::theme::ColorToken::Focus);
    commands
        .iter()
        .rev()
        .find_map(|c| match c {
            RenderCommand::StrokeRect { rect, color, .. }
                if *color == focus && rect.min_x() >= pane_left(window) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the focused row wears the focus ring")
}

/// The page's scroll viewport: the clip band the sheet draws inside its own
/// bounds.
fn clip_in_panel(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> RectF {
    let panel = sheet_panel(commands, app, window);
    commands
        .iter()
        .rev()
        .find_map(|c| match c {
            RenderCommand::ClipRect(rect)
                if rect.min_x() >= panel.min_x() && rect.max_x() <= panel.max_x() =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the page scrolls in a clipped viewport")
}

/// Every `Bg`-coloured fill the frame paints.
fn bg_fills(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
    let bg = app.theme.color(goble_ui::theme::ColorToken::Bg);
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. } if *color == bg => Some(*rect),
            _ => None,
        })
        .collect()
}

/// The sheet's own background: the `Bg` fill the dialog centers at
/// `PANEL_WIDTH` x `PANEL_HEIGHT`, the height capped at 90% of the viewport and
/// the width at the viewport's own.
fn sheet_panel(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> RectF {
    let width = crate::ui::settings::PANEL_WIDTH.min(window.x);
    let height = crate::ui::settings::PANEL_HEIGHT.min(window.y * 0.9);
    bg_fills(commands, app)
        .into_iter()
        .find(|rect| {
            rect.width() == width
                && rect.height() == height
                && rect.min_x() == (window.x - width) * 0.5
                && rect.min_y() == (window.y - height) * 0.5
        })
        .unwrap_or_else(|| {
            panic!(
                "no sheet of {width}x{height} centered in {window:?}: {:?}",
                bg_fills(commands, app)
            )
        })
}

/// The x of the content column's left edge.
fn pane_left(window: Vector2F) -> f32 {
    (window.x - crate::ui::settings::PANEL_WIDTH) * 0.5
        + crate::ui::settings::NAV_WIDTH
        + crate::ui::settings::RULE_WIDTH
}

/// The switch's 44x24 track, the only one inside the page.
fn switch_track(commands: &[RenderCommand], window: Vector2F) -> Option<RectF> {
    commands.iter().find_map(|c| match c {
        RenderCommand::FillRect { rect, .. }
            if rect.width() == 44.0 && rect.height() == 24.0 && rect.min_x() >= pane_left(window) =>
        {
            Some(*rect)
        }
        _ => None,
    })
}

/// Lay out and paint one frame with the pointer at `at`, so a scroll region
/// records that the wheel belongs to it, and return its commands.
fn frame_at(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    window: Vector2F,
    at: Vector2F,
) -> Vec<RenderCommand> {
    let constraint = SizeConstraint::loose(window);
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    paint_ctx.cursor_inside = true;
    paint_ctx.cursor_position = at;
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
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
    press(&mut root, &app, "Enter"); // commit the name
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
    press(&mut root, &app, "Enter"); // commit the name
    press(&mut root, &app, "ArrowDown"); // the secret-value field
    press(&mut root, &app, "Enter");
    for c in ["s", "k", "-", "1"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Enter"); // commit the value
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
