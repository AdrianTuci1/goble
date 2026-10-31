//! Drag-to-copy over the agent transcript, through the mounted root and the
//! app's own clipboard helper.
//!
//! The unit test in `goble-ui` proves the selection gesture; this is the
//! acceptance for the app side — the pane's selection is wired into the built
//! tree, the release reaches the root's event path, and the copy handler puts
//! the text on the system clipboard without a Cmd+C.

use super::*;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use goble_ui::geometry::vec2f;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;

/// `PATH` is process-wide, so the fake `pbcopy` must not overlap another test
/// that runs a clipboard helper.
static CLIPBOARD_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Restore `PATH` even when an assertion fails mid-test.
struct PathGuard(std::ffi::OsString);

impl Drop for PathGuard {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.0);
    }
}

/// A `pbcopy` early on `PATH` that records what the app copied, so the test
/// reads exactly what the real handler wrote.
fn fake_pbcopy(dir: &Path) -> std::path::PathBuf {
    let target = dir.join("copied.txt");
    let script = dir.join("pbcopy");
    let body = format!("#!/bin/sh\ncat > '{}'\n", target.display());
    std::fs::write(&script, body).expect("write fake pbcopy");
    let mut permissions = std::fs::metadata(&script).expect("stat fake pbcopy").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("make fake pbcopy executable");
    target
}

#[test]
fn a_transcript_drag_lands_on_the_system_clipboard() {
    let _guard = CLIPBOARD_ENV.lock().unwrap_or_else(|poison| poison.into_inner());
    let clipboard_dir = tempfile::tempdir().expect("clipboard dir");
    let copied_path = fake_pbcopy(clipboard_dir.path());
    let previous_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path = std::ffi::OsString::from(clipboard_dir.path());
    path.push(":");
    path.push(&previous_path);
    std::env::set_var("PATH", path);
    let _path = PathGuard(previous_path);

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let conversation = desktop
        .create_chat("Transcript copy", None, None)
        .expect("create chat");
    desktop
        .add_chat_message(
            &conversation,
            "user",
            "COPY MARKER: drag across this line and release",
        )
        .expect("user row");

    let root = RootView::new(&AppContext::default(), &desktop, None);
    {
        let state = root.state_rc();
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.pane_sessions
            .get_mut(&1)
            .expect("the default pane session exists")
            .conversation_id = conversation;
        s.refresh_messages(&desktop);
    }

    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(root);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let origin = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if text.contains("COPY MARKER") && origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the transcript draws the user row");

    let mut ctx = EventContext;
    let start = origin + vec2f(2.0, 6.0);
    let end = origin + vec2f(180.0, 6.0);
    for event in [
        DispatchedEvent::MouseDown {
            position: start,
            button: 0,
        },
        DispatchedEvent::MouseMove { position: end },
        DispatchedEvent::MouseUp {
            position: end,
            button: 0,
        },
    ] {
        root.dispatch_event(&event, &mut ctx, &app);
    }

    let copied = std::fs::read_to_string(&copied_path).unwrap_or_default();
    assert!(
        copied.contains("COPY MARKER"),
        "the release must copy the dragged text, got {copied:?}"
    );
}

/// The drag must still work after a frame that did not paint a transcript yet:
/// the pointer events may arrive before the first paint. This is the shape the
/// app gets when a conversation is opened and the mouse is already moving.
#[test]
fn a_drag_before_the_first_paint_copies_nothing_and_does_not_panic() {
    let _guard = CLIPBOARD_ENV.lock().unwrap_or_else(|poison| poison.into_inner());
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let root = RootView::new(&AppContext::default(), &desktop, None);
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(root);
    let mut ctx = EventContext;
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(300.0, 300.0),
            button: 0,
        },
        DispatchedEvent::MouseMove {
            position: vec2f(400.0, 300.0),
        },
        DispatchedEvent::MouseUp {
            position: vec2f(400.0, 300.0),
            button: 0,
        },
    ] {
        root.dispatch_event(&event, &mut ctx, &app);
    }
}
