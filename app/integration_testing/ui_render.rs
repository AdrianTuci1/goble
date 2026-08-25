//! Smoke tests for the whole app shell: mounting the real [`RootView`] over a
//! live [`DesktopState`] and laying it out + painting it headlessly must not
//! panic and must emit render commands. This exercises the seam where the
//! backend data flows into the hot-reloadable element tree (`build_ui`).

mod common;

use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_desktop_service::DesktopState;
use goble_ui::elements::AppContext;
use goble_ui::test_util::{command_counts, render_element, RenderCommandCounts};
use goble_ui::{vec2f, Element};

fn render(desktop: &Arc<DesktopState>) -> RenderCommandCounts {
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(RootView::new(&app, Some(Arc::clone(desktop)), None));
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
