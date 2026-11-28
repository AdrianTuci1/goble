//! A3: the environment a cloud-agent conversation runs on, chosen where the
//! prompt is submitted and remembered per conversation.
//!
//! The direction splits on routing: a conversation that runs on a worker has an
//! environment to choose — the pane draws the control beside the send
//! affordance — and a local conversation has none, so its pane draws no such
//! control at all. The choice is persisted on the conversation
//! (`chats.medium_id`, the same per-conversation idiom as `workspace_routing`),
//! so two conversations never share one environment and it survives a restart.
//!
//! The model half of A3 stays `[~]`: a per-turn model chosen here would have to
//! reach the worker, and the worker builds its provider from its own secrets and
//! `LLM_PROVIDER`/`LLM_MODEL` (`goblin-worker`'s `llm_factory`) with no model
//! field on the turn request (`goble-harness-types::HarnessTurn`) to override it
//! with. Nothing is drawn for a control that could not be honoured, and nothing
//! is sent that the worker would ignore.

use super::*;
use crate::ui::{PaneKind, SplitDir, WorkspaceRouting};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::vec2f;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;
use goble_ui::EventContext;

/// A mounted root over a live backend, handing the backend back so a case can
/// pair a worker and route a conversation before the frame reads either.
fn root_with_backend() -> (RootView, Arc<DesktopState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let bus = CollectingEventBus::new();
    let view = RootView::new(&AppContext::default(), &desktop, Some(bus.clone()));
    (view, desktop, dir)
}

/// A backend holding a paired worker, the way a paired VPS reaches the app (the
/// worker row plus the `Paired` report its channel confirms with). No live host
/// exists here: the pool's own answer is what the pane resolves.
fn pair_worker(desktop: &DesktopState, id: &str) {
    let worker = goble_core::worker::WorkerId(id.to_string());
    desktop
        .add_worker(
            worker.clone(),
            "vps".to_string(),
            "ws://vps:8787/ws".to_string(),
        )
        .expect("register the worker");
    desktop.handle_worker_message(&worker, goble_core::protocol::WorkerMessage::Paired);
}

/// A conversation in the store, routed where `routing` says.
fn conversation(desktop: &DesktopState, title: &str, routing: &str) -> String {
    let id = desktop
        .create_chat(title, None, None)
        .expect("create the conversation");
    desktop
        .set_chat_workspace_routing(&id, Some(routing))
        .expect("persist the routing");
    id
}

/// Give `pane_id` a conversation of its own and refresh, the way binding a
/// conversation to a pane does.
fn bind(root: &RootView, desktop: &DesktopState, pane_id: u64, conv: &str) {
    {
        let state = root.state_rc();
        let mut state = state.borrow_mut();
        state
            .pane_sessions
            .entry(pane_id)
            .or_default()
            .conversation_id = conv.to_string();
    }
    root.state_rc().borrow_mut().refresh_messages(desktop);
}

/// Split the active pane to the right, returning the new pane's id.
fn split_right(root: &RootView, kind: PaneKind) -> u64 {
    let state = root.state_rc();
    let mut state = state.borrow_mut();
    let (space, pane) = (state.active_space, state.active_pane_id);
    let mut next = state.next_pane_id;
    let second = state.spaces[space]
        .split_with_kind(pane, SplitDir::Vertical, &mut next, kind)
        .expect("open a pane beside the active one");
    state.next_pane_id = next;
    second
}

/// One layout + paint pass with the pointer placed at `pointer`, which is where
/// hover lives: the tree is rebuilt every frame, so an element reads the cursor
/// at paint. Mirrors the frame the app draws.
fn render(
    root: &mut RootView,
    app: &AppContext,
    pointer: Option<(f32, f32)>,
) -> Vec<RenderCommand> {
    use goble_ui::elements::{LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::render::Renderer;

    let _ = root.layout(
        SizeConstraint::loose(vec2f(1024.0, 768.0)),
        &mut LayoutContext,
        app,
    );
    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
    if let Some((x, y)) = pointer {
        paint_ctx.cursor_position = vec2f(x, y);
        paint_ctx.cursor_inside = true;
    }
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// A full press+release at `pos`, re-rendering with the pointer held at `pos`
/// between the two (the real app redraws between down and up, and a surface
/// that only exists under the pointer is drawn by that frame).
fn click(root: &mut RootView, app: &AppContext, pos: (f32, f32)) {
    let mut ctx = EventContext;
    let down = DispatchedEvent::MouseDown {
        position: vec2f(pos.0, pos.1),
        button: 0,
    };
    let _ = root.dispatch_event(&down, &mut ctx, app);
    let _ = render(root, app, Some(pos));
    let up = DispatchedEvent::MouseUp {
        position: vec2f(pos.0, pos.1),
        button: 0,
    };
    let _ = root.dispatch_event(&up, &mut ctx, app);
}

/// The center of the first `name` icon drawn inside the pane area (right of the
/// sidebar), so a glyph the sidebar also draws is not mistaken for a pane's.
fn pane_icon_center(cmds: &[RenderCommand], name: &str) -> Option<(f32, f32)> {
    cmds.iter().find_map(|c| match c {
        RenderCommand::DrawIcon {
            origin,
            name: n,
            size,
            ..
        } if n == name && origin.x >= crate::ui::SIDEBAR_WIDTH => {
            Some((origin.x + size / 2.0, origin.y + size / 2.0))
        }
        _ => None,
    })
}

/// The center of the pane's environment control: the icon-only 16pt glyph at the
/// composer card's context band. The empty-state landing draws its own mark at
/// 28pt, so the size is what tells the control from the illustration.
fn environment_control_center(cmds: &[RenderCommand], name: &str) -> Option<(f32, f32)> {
    cmds.iter().find_map(|c| match c {
        RenderCommand::DrawIcon {
            origin,
            name: n,
            size,
            ..
        } if n == name && origin.x >= crate::ui::SIDEBAR_WIDTH && (*size - 16.0).abs() < 0.01 => {
            Some((origin.x + size / 2.0, origin.y + size / 2.0))
        }
        _ => None,
    })
}

/// How many times the pane area drew the environment control's glyph (the 16pt
/// icon-only control, not the landing's larger mark).
fn environment_control_count(cmds: &[RenderCommand], name: &str) -> usize {
    cmds.iter()
        .filter(|c| {
            matches!(c, RenderCommand::DrawIcon { origin, name: n, size, .. }
                if n == name
                    && origin.x >= crate::ui::SIDEBAR_WIDTH
                    && (*size - 16.0).abs() < 0.01)
        })
        .count()
}

/// Every place inside the pane area (right of the sidebar) that drew `text`.
fn pane_text_positions(cmds: &[RenderCommand], text: &str) -> Vec<(f32, f32)> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText {
                origin, text: run, ..
            } if run == text && origin.x >= crate::ui::SIDEBAR_WIDTH => Some((origin.x, origin.y)),
            _ => None,
        })
        .collect()
}

/// The pane-scoped text of a frame.
fn pane_texts(cmds: &[RenderCommand]) -> Vec<String> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. }
                if origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// Take the first-run overlays out of the frame so what is drawn is the panes.
fn clear_overlays(state: &Rc<RefCell<UiState>>) {
    let mut state = state.borrow_mut();
    state.show_workspace_choice = false;
    state.show_llm_key_banner = false;
    state.show_onboarding_tip = false;
}

/// A layout restored on startup keeps the tab it was left on, and with it that
/// tab's environment: the selector and the sidebar start on the tab's medium
/// instead of the environment tree's default.
#[test]
fn a_restored_tab_starts_on_its_own_environment() {
    use crate::ui::{Pane, Space};

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let app = AppContext::default();

    {
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.show_onboarding_tip = false;
        s.sidebar_visible = true;
        let id = s.next_pane_id;
        s.next_pane_id += 1;
        s.spaces.push(
            Space::new(
                "Remote",
                Pane::Leaf {
                    id,
                    kind: PaneKind::Terminal,
                },
            )
            .with_medium("remote"),
        );
        s.active_space = s.spaces.len() - 1;
        s.active_pane_id = id;
        s.save_panes(&desktop);
    }

    let view = RootView::new(&app, &desktop, None);
    assert_eq!(
        view.media_state_rc().borrow().selected_medium_id(),
        "remote",
        "the restored tab is the environment the window starts on"
    );
    assert_eq!(
        view.state_rc().borrow().workspace_routing,
        Some(WorkspaceRouting::Remote),
        "and its turns route there"
    );
}
