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
    assert_eq!(SettingsCategory::ALL.len(), 6);
    let labels: Vec<&str> = SettingsCategory::ALL.iter().map(|c| c.label()).collect();
    assert_eq!(
        labels,
        vec![
            "Appearance",
            "Mouse",
            "Editor & Input",
            "Agent & Approval",
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
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use std::sync::Arc;

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let root = RootView::new(&AppContext::default(), &desktop, None);
    let state = root.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        s.settings_overlay_open = true;
        s.settings_category = category;
    }
    (Box::new(root) as Box<dyn Element>, state, dir)
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

