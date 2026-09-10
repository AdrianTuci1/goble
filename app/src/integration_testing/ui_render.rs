//! Smoke tests for the whole app shell: mounting the real [`RootView`] over a
//! live [`DesktopState`] and laying it out + painting it headlessly must not
//! panic and must emit render commands. This exercises the seam where the
//! backend data flows into the app-owned element tree (`build_ui`).

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::media::MediaState;
use goble_app::root_view::RootView;
use goble_app::state::UiState;
use goble_app::ui::AppTab;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::AppContext;
use goble_ui::platform::WindowControl;
use goble_ui::test_util::{command_counts, render_element, RenderCommandCounts};
use goble_ui::{vec2f, Element, SettingsPage};

fn render(desktop: &Arc<DesktopState>) -> RenderCommandCounts {
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, desktop, None));
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    command_counts(&commands)
}

/// Render the whole shell with a given first-run flag forced on, to exercise
/// the modal overlay path headlessly (no browser for this native app).
fn render_with_flag(
    desktop: &Arc<DesktopState>,
    set: impl FnOnce(&mut UiState),
) -> RenderCommandCounts {
    let app = AppContext::default();
    let view = RootView::new(&app, desktop, None);
    set(&mut view.state_rc().borrow_mut());
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    command_counts(&commands)
}

#[test]
fn full_app_renders_from_empty_backend() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render(&desktop);
    assert!(counts.fill_rect > 0, "shell should paint backgrounds");
    assert!(counts.draw_text > 0, "shell should paint text");
}

#[test]
fn full_app_renders_with_chat_data() {
    let (desktop, _dir) = common::desktop_state();
    let chat_id = desktop
        .create_chat("Demo", None, None)
        .expect("create chat");
    desktop
        .add_chat_message(&chat_id, "user", "Salut!")
        .expect("add user message");
    desktop
        .add_chat_message(&chat_id, "assistant", "Bine ai venit!")
        .expect("add assistant message");

    let counts = render(&desktop);
    assert!(counts.fill_rect > 0, "shell should paint backgrounds");
    assert!(counts.draw_text > 0, "shell should paint text");
}

#[test]
fn full_app_renders_inline_key_error() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render_with_flag(&desktop, |s| s.show_llm_key_banner = true);
    assert!(
        counts.fill_rect > 0,
        "the inline key error should paint its panel"
    );
    assert!(
        counts.draw_text > 0,
        "the inline key error should paint its label"
    );
}

#[test]
fn full_app_renders_workspace_choice_overlay() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render_with_flag(&desktop, |s| s.show_workspace_choice = true);
    assert!(
        counts.fill_rect > 0,
        "workspace choice overlay should paint"
    );
    assert!(
        counts.draw_text > 0,
        "workspace choice overlay should paint its label"
    );
}

#[test]
fn toggle_right_sidebar_action_flips_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        None,
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );

    assert!(!state.borrow().right_sidebar_open, "sidebar starts hidden");
    (actions.on_toggle_right_sidebar.borrow_mut())();
    assert!(
        state.borrow().right_sidebar_open,
        "toggle opens the sidebar"
    );
    (actions.on_toggle_right_sidebar.borrow_mut())();
    assert!(
        !state.borrow().right_sidebar_open,
        "toggling again hides the sidebar"
    );
}

#[test]
fn settings_navigate_and_back_flip_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        None,
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );

    assert_eq!(state.borrow().current_tab, AppTab::Chat);
    (actions.on_settings_navigate.borrow_mut())(SettingsPage::Llm);
    assert_eq!(state.borrow().settings_page, SettingsPage::Llm);
    (actions.on_settings_back.borrow_mut())();
    assert_eq!(
        state.borrow().current_tab,
        AppTab::Chat,
        "back returns to chat"
    );
    assert_eq!(
        state.borrow().settings_page,
        SettingsPage::Llm,
        "back keeps the last selected settings page"
    );
}

#[test]
fn pane_layout_persists_across_restart() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store_path = dir.path().join("store.sqlite");

    // First run: split the active pane to the right, then "quit" the app. The
    // on_split_right action persists the pane layout via the DesktopState store.
    {
        let desktop = Arc::new(DesktopState::new(
            Store::open(&store_path).expect("open store"),
            ThreadStore::new(dir.path().join("threads")).expect("open thread store"),
        ));
        let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            Rc::new(RefCell::new(MediaState::mock())),
            WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );
        (actions.on_split_right.borrow_mut())();

        let split = state.borrow();
        assert_eq!(split.spaces[0].root.max_id(), 3, "split allocates a new leaf");
        assert_eq!(split.active_pane_id, 3, "new pane becomes active");
    }

    // Reopen the same sqlite store: the restored layout must match what was
    // persisted on the first run.
    {
        let desktop = Arc::new(DesktopState::new(
            Store::open(&store_path).expect("reopen store"),
            ThreadStore::new(dir.path().join("threads")).expect("reopen thread store"),
        ));
        let state = UiState::from_desktop(&desktop);
        assert_eq!(state.spaces.len(), 1, "one space restored");
        assert_eq!(state.spaces[0].root.max_id(), 3, "split tree restored");
        assert!(
            state.spaces[0].root.contains_leaf(1) && state.spaces[0].root.contains_leaf(3),
            "both leaves restored"
        );
        assert_eq!(state.active_pane_id, 3, "active pane restored");
        assert!(state.next_pane_id > 3, "id counter recovered above max id");
    }
}

#[test]
fn toggle_dark_mode_updates_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        None,
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );

    assert!(!state.borrow().settings_dark_mode);
    (actions.on_toggle_dark_mode.borrow_mut())(true);
    assert!(state.borrow().settings_dark_mode);
}

/// Index of the first `DrawText` command whose text contains `needle`.
fn text_index(commands: &[goble_ui::render::RenderCommand], needle: &str) -> Option<usize> {
    commands.iter().position(|c| {
        matches!(c, goble_ui::render::RenderCommand::DrawText { text, .. } if text.contains(needle))
    })
}

/// Every workspace in `state.spaces` is drawn as its own chip in the toolbar,
/// so adding a workspace shows it *next to* the current one.
#[test]
fn topbar_lists_every_workspace() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    {
        let mut state = state_rc.borrow_mut();
        let id = state.next_pane_id;
        state.next_pane_id += 1;
        state.spaces.push(goble_app::ui::Space::new(
            "Workspace Two",
            goble_app::ui::Pane::Leaf {
                id,
                kind: goble_app::ui::PaneKind::Terminal,
            },
        ));
    }
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    assert!(
        text_index(&commands, "Space 1").is_some(),
        "the current workspace chip is drawn"
    );
    assert!(
        text_index(&commands, "Workspace Two").is_some(),
        "the added workspace is drawn next to the current one"
    );
}

/// The Settings→Appearance HSV pickers paint at their own layout origin (the
/// panel column), not at the window origin where they would smear over the
/// sidebar and the toolbar.
#[test]
fn settings_color_pickers_paint_inside_the_panel() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    {
        let mut state = state_rc.borrow_mut();
        state.settings_overlay_open = true;
        state.settings_category = goble_app::ui::SettingsCategory::Appearance;
    }
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

    // The SV square is drawn as a column of fade-right rows (see ColorPicker).
    let rows: Vec<f32> = commands
        .iter()
        .filter_map(|c| match c {
            goble_ui::render::RenderCommand::FillRectFadeRight { rect, .. } => Some(rect.min_x()),
            _ => None,
        })
        .collect();
    assert!(!rows.is_empty(), "the appearance pane draws its HSV pickers");
    for x in rows {
        assert!(
            x > 300.0,
            "picker painted at x={x}, outside the settings panel column"
        );
    }
}

/// Origin of the first `DrawText` command whose text contains `needle`.
fn text_origin(
    commands: &[goble_ui::render::RenderCommand],
    needle: &str,
) -> Option<goble_ui::Vector2F> {
    commands.iter().find_map(|c| match c {
        goble_ui::render::RenderCommand::DrawText { origin, text, .. } if text.contains(needle) => {
            Some(*origin)
        }
        _ => None,
    })
}

/// The toolbar is layered above the shell body so its trays paint over the
/// sidebar and the pane surface, but it still reserves its height in the body
/// column: every body surface must start below the toolbar band, or the
/// toolbar would cover the sidebar's search field.
#[test]
fn shell_body_paints_below_the_toolbar() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, &desktop, None));
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let topbar_height = goble_app::ui::shell::TOPBAR_HEIGHT;

    let chip = text_origin(&commands, "Space 1").expect("workspace chip");
    assert!(
        chip.y < topbar_height,
        "workspace chips paint inside the toolbar (y={})",
        chip.y
    );

    for needle in ["Search", "Ask anything to get started"] {
        let y = text_origin(&commands, needle)
            .unwrap_or_else(|| panic!("{needle} is drawn"))
            .y;
        assert!(
            y >= topbar_height,
            "{needle} paints at y={y}, inside the toolbar band (height {topbar_height})"
        );
    }
}

/// The toolbar's trays paint above the shell surface: the "+ ▾" environment
/// menu is drawn after the sidebar and the pane content, so nothing covers it.
#[test]
fn environment_menu_paints_above_the_shell_surface() {
    let (desktop, _dir) = common::desktop_state();
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state_rc = view.state_rc();
    *state_rc.borrow().env_selector_open.borrow_mut() = true;
    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

    let sidebar = text_index(&commands, "Search").expect("sidebar search field");
    let pane = text_index(&commands, "Ask anything to get started")
        .expect("pane empty state");
    let menu = text_index(&commands, "New environment").expect("environment menu item");
    assert!(
        menu > sidebar,
        "the environment tray ({menu}) must paint above the sidebar ({sidebar})"
    );
    assert!(
        menu > pane,
        "the environment tray ({menu}) must paint above the pane surface ({pane})"
    );
}

