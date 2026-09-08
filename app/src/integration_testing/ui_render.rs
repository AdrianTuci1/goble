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
fn full_app_renders_key_banner_overlay() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render_with_flag(&desktop, |s| s.show_llm_key_banner = true);
    assert!(
        counts.fill_rect > 0,
        "banner overlay should paint its panel"
    );
    assert!(
        counts.draw_text > 0,
        "banner overlay should paint its label"
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
    );

    assert!(!state.borrow().settings_dark_mode);
    (actions.on_toggle_dark_mode.borrow_mut())(true);
    assert!(state.borrow().settings_dark_mode);
}
