//! Smoke tests for the whole app shell: mounting the real [`RootView`] over a
//! live [`DesktopState`] and laying it out + painting it headlessly must not
//! panic and must emit render commands. This exercises the seam where the
//! backend data flows into the hot-reloadable element tree (`build_ui`).

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::actions::make_actions;
use goble_app::hot_ui::AppTab;
use goble_app::root_view::RootView;
use goble_app::state::UiState;
use goble_desktop_service::DesktopState;
use goble_ui::elements::AppContext;
use goble_ui::test_util::{command_counts, render_element, RenderCommandCounts};
use goble_ui::{vec2f, Element, SettingsPage};

fn render(desktop: &Arc<DesktopState>) -> RenderCommandCounts {
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, Some(Arc::clone(desktop)), None));
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    command_counts(&commands)
}

/// Render the whole shell with a given first-run flag forced on, to exercise
/// the modal overlay path headlessly (no browser for this native app).
fn render_with_flag(desktop: &Arc<DesktopState>, set: impl FnOnce(&mut UiState)) -> RenderCommandCounts {
    let app = AppContext::default();
    let mut view = RootView::new(&app, Some(Arc::clone(desktop)), None);
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
    let chat_id = desktop.create_chat("Demo", None, None).expect("create chat");
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
    assert!(counts.fill_rect > 0, "banner overlay should paint its panel");
    assert!(counts.draw_text > 0, "banner overlay should paint its label");
}

#[test]
fn full_app_renders_workspace_choice_overlay() {
    let (desktop, _dir) = common::desktop_state();
    let counts = render_with_flag(&desktop, |s| s.show_workspace_choice = true);
    assert!(counts.fill_rect > 0, "workspace choice overlay should paint");
    assert!(counts.draw_text > 0, "workspace choice overlay should paint its label");
}

#[test]
fn toggle_right_sidebar_action_flips_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(Rc::clone(&state), None);

    assert!(!state.borrow().right_sidebar_open, "sidebar starts hidden");
    (actions.on_toggle_right_sidebar.borrow_mut())();
    assert!(state.borrow().right_sidebar_open, "toggle opens the sidebar");
    (actions.on_toggle_right_sidebar.borrow_mut())();
    assert!(
        !state.borrow().right_sidebar_open,
        "toggling again hides the sidebar"
    );
}

#[test]
fn settings_navigate_and_back_flip_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(Rc::clone(&state), None);

    assert_eq!(state.borrow().current_tab, AppTab::Chat);
    (actions.on_settings_navigate.borrow_mut())(SettingsPage::Llm);
    assert_eq!(state.borrow().settings_page, SettingsPage::Llm);
    (actions.on_settings_back.borrow_mut())();
    assert_eq!(state.borrow().current_tab, AppTab::Chat, "back returns to chat");
    assert_eq!(
        state.borrow().settings_page,
        SettingsPage::Llm,
        "back keeps the last selected settings page"
    );
}

#[test]
fn toggle_dark_mode_updates_state() {
    let state = Rc::new(RefCell::new(UiState::mock()));
    let actions = make_actions(Rc::clone(&state), None);

    assert!(!state.borrow().settings_dark_mode);
    (actions.on_toggle_dark_mode.borrow_mut())(true);
    assert!(state.borrow().settings_dark_mode);
}
