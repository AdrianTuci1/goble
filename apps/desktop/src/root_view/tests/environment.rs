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

#[test]
fn each_pane_draws_the_environment_its_work_runs_on() {
    let (mut root, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let here = conversation(&desktop, "Run it here", "local");
    let there = conversation(&desktop, "Ship it remotely", "remote");

    // Two panes in one space: the frame is proven willing to draw the control,
    // so the pane that draws none is the pane's own doing and not the frame's.
    let second = split_right(&root, PaneKind::Chat);
    bind(&root, &desktop, 1, &here);
    bind(&root, &desktop, second, &there);

    assert_eq!(
        root.state_rc().borrow().pane_environment(1),
        None,
        "a local conversation has no environment of its own to choose"
    );
    assert_eq!(
        root.state_rc().borrow().pane_environment(second),
        Some("remote".to_string()),
        "a conversation routed to a worker holds the environment its routing implies"
    );

    clear_overlays(&root.state_rc());
    // Each pane is given a model of its own, so the model control the worker
    // pane must not draw is one that would otherwise name a real model: the
    // gating is the pane's, not an empty label's.
    {
        let state = root.state_rc();
        let mut state = state.borrow_mut();
        state.pane_controls_mut(1).model = "local-model".to_string();
        state.pane_controls_mut(second).model = "worker-model".to_string();
    }
    let app = AppContext::default();
    let cmds = render(&mut root, &app, None);

    // The environment control is the pane's own, so each pane names where its
    // work runs: the computer for the local conversation and the cloud for the
    // worker one, each once in the pane area.
    assert!(
        pane_icon_center(&cmds, "environment-cloud").is_some(),
        "the remote conversation's pane draws the cloud: {cmds:?}"
    );
    assert!(
        pane_icon_center(&cmds, "environment-computer").is_some(),
        "the local conversation's pane draws the computer: {cmds:?}"
    );
    // The control is icon-only — the reference's monitor/cloud glyph at the
    // context band's right end — so the pane draws each environment's own mark
    // once, and no environment label at rest.
    assert_eq!(
        environment_control_count(&cmds, "environment-cloud"),
        1,
        "one cloud control for the worker pane"
    );
    assert_eq!(
        environment_control_count(&cmds, "environment-computer"),
        1,
        "one computer control for the local pane"
    );
    let texts = pane_texts(&cmds);
    assert!(
        !texts.iter().any(|t| t == "Remote (worker)"),
        "the control is icon-only at rest: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Local"),
        "the local control is icon-only too: {texts:?}"
    );

    // The name the control carries lives in its tooltip: a pointer over the
    // worker pane's control names the environment it submits to.
    let worker_control = environment_control_center(&cmds, "environment-cloud")
        .expect("the worker pane draws its environment control");
    let hovered = render(&mut root, &app, Some(worker_control));
    assert!(
        pane_texts(&hovered)
            .iter()
            .any(|t| t == "Environment · Remote (worker)"),
        "the control names its environment on hover: {:?}",
        pane_texts(&hovered)
    );
    // The model is the client's own choice, so it is drawn only where the
    // client's choice is the one the turn runs on: the local conversation's
    // pane draws its model control, and the worker conversation's pane draws
    // none, because the remote turn carries no model for the control to set.
    assert!(
        texts.iter().any(|t| t == "local-model"),
        "the local conversation's pane draws its model control: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "worker-model"),
        "the worker conversation's pane draws no model control: {texts:?}"
    );
}

#[test]
fn the_chosen_environment_is_remembered_on_the_conversation() {
    let (mut root, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let there = conversation(&desktop, "Ship it remotely", "remote");
    bind(&root, &desktop, 1, &there);
    let app = AppContext::default();

    clear_overlays(&root.state_rc());
    let cmds = render(&mut root, &app, None);
    let control = environment_control_center(&cmds, "environment-cloud")
        .expect("the environment control is drawn");

    // The control opens the host's menu, the same popup every other control
    // opens.
    click(&mut root, &app, control);
    let cmds = render(&mut root, &app, None);
    assert!(
        *root
            .state_rc()
            .borrow()
            .pane_controls(1)
            .harness_menu_open
            .borrow(),
        "a press on the pill opens the pane's environment menu"
    );
    // The worker row carries the reference's description line, one ellipsized
    // line under the label: the app passes it, the casing cuts it.
    let texts = pane_texts(&cmds);
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("Clones this repo") && t.ends_with('…') && !t.contains('\n')),
        "the worker row describes what picking it does: {texts:?}"
    );
    let row = pane_text_positions(&cmds, "Local")
        .into_iter()
        .next()
        .map(|(x, y)| (x + 8.0, y + 6.0))
        .expect("the open menu lists the environments the app has");

    // Choosing one settles it on the conversation and on the pane.
    click(&mut root, &app, row);
    assert_eq!(
        desktop
            .get_chat_medium(&there)
            .expect("read the conversation's environment"),
        Some("local".to_string()),
        "the choice is persisted on the conversation it was made in"
    );
    assert_eq!(
        root.state_rc().borrow().pane_environment(1),
        Some("local".to_string()),
        "and the pane holds it, which is what the submit reads"
    );
    let cmds = render(&mut root, &app, None);
    // The control is icon-only and draws the chosen environment's own mark: the
    // local monitor once the pane's turns run on the local medium. The empty
    // local landing may draw a larger mark, so the control is the 16pt one.
    assert_eq!(
        environment_control_count(&cmds, "environment-computer"),
        1,
        "the control draws the computer the turns now run on: {cmds:?}"
    );
    assert_eq!(
        environment_control_count(&cmds, "environment-cloud"),
        0,
        "and no longer the cloud it was routed to: {cmds:?}"
    );
    let control = environment_control_center(&cmds, "environment-computer")
        .expect("the local environment control is drawn");
    let hovered = render(&mut root, &app, Some(control));
    assert!(
        pane_texts(&hovered)
            .iter()
            .any(|t| t == "Environment · Local"),
        "the control names the chosen environment on hover: {:?}",
        pane_texts(&hovered)
    );

    // A second conversation keeps its own: the environment is per conversation,
    // not per window.
    let another = conversation(&desktop, "Another remote thread", "remote");
    let second = split_right(&root, PaneKind::Chat);
    bind(&root, &desktop, second, &another);
    assert_eq!(
        root.state_rc().borrow().pane_environment(second),
        Some("remote".to_string()),
        "a conversation nothing was chosen for keeps the environment its routing implies"
    );
    assert_eq!(
        desktop.get_chat_medium(&another).unwrap(),
        None,
        "and nothing was written onto it"
    );

    // A fresh client over the same backend draws the same choice: it is the
    // conversation's row, not this run's memory.
    let reopened = RootView::new(&AppContext::default(), &desktop, None);
    bind(&reopened, &desktop, 1, &there);
    clear_overlays(&reopened.state_rc());
    let mut reopened: Box<dyn Element> = Box::new(reopened);
    let cmds = render_element(&mut reopened, vec2f(1024.0, 768.0), &AppContext::default());
    assert_eq!(
        environment_control_count(&cmds, "environment-computer"),
        1,
        "the choice survives a new client: the control draws the chosen computer"
    );
}

#[test]
fn the_submitted_turn_carries_the_chosen_environment() {
    let (mut root, desktop, _dir) = root_with_backend();
    pair_worker(&desktop, "vps");
    let there = conversation(&desktop, "Ship it remotely", "remote");
    bind(&root, &desktop, 1, &there);
    let app = AppContext::default();
    clear_overlays(&root.state_rc());

    // Choose the pane's own environment through its control.
    let cmds = render(&mut root, &app, None);
    let control = environment_control_center(&cmds, "environment-cloud")
        .expect("the environment control is drawn");
    click(&mut root, &app, control);
    let cmds = render(&mut root, &app, None);
    let row = pane_text_positions(&cmds, "Local")
        .into_iter()
        .next()
        .map(|(x, y)| (x + 8.0, y + 6.0))
        .expect("the open menu lists the environments the app has");
    click(&mut root, &app, row);

    // The window's own environment is moved to another one first, so the value
    // that follows is the pane's and not the window's: `send_agent_prompt` reads
    // the pane's environment (the same `pane_environment`), never the window's
    // selection.
    root.media_state_rc().borrow_mut().select_medium("remote");
    let medium = root
        .state_rc()
        .borrow()
        .pane_environment(1)
        .expect("the pane holds the environment the submit reads");
    assert_eq!(
        medium, "local",
        "the chosen environment, not the window's and not the routing's"
    );

    // A configured model, so the submit is a real one: without one the pane
    // shows its notice band and sends nothing at all.
    {
        let state = root.state_rc();
        let mut state = state.borrow_mut();
        state.settings_llm_provider = "mock".to_string();
        state.settings_llm_api_key = "test-key".to_string();
        state.models = vec!["mock".to_string()];
        state.selected_model = "mock".to_string();
    }

    // Submitting at the pane takes the remote route: the app has no remote
    // transport wired, so it fails loudly instead of running the turn on this
    // machine's own environment. That is the state the pane draws.
    let actions = crate::actions::make_actions(
        root.state_rc(),
        Some(Arc::clone(&desktop)),
        root.media_state_rc(),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    (actions.on_send_message.borrow_mut())("run the tests".to_string());
    let rows = desktop
        .list_chat_messages(&there)
        .expect("read the conversation");
    let reported = rows
        .iter()
        .map(|r| (r.role.clone(), r.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        reported.len(),
        1,
        "no turn ran here: the submit started nothing on this machine: {reported:?}"
    );
    assert!(
        reported[0].0 == "assistant" && reported[0].1.contains("routed remote"),
        "the submit went to the remote route and said so: {reported:?}"
    );

    // The turn the remote route builds, with a client attached where the app has
    // none: the paired worker's own client (the daemon the desktop drives stands
    // in for it — no live host exists in this run). Its stream is read from
    // before the run, since that is what a client of a remote daemon sees.
    let mut events = desktop.daemon_client().subscribe();
    desktop.register_harness(Arc::new(goble_harness_runtime::MockHarness::new(
        goble_harness_types::HarnessId::new("cli-a"),
        "all green",
    )));
    let client = desktop.daemon_client();
    let model = crate::daemon::DaemonModel::new(Arc::clone(&desktop)).with_remote(client);
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();
    let handle = model
        .run_turn(crate::daemon::TurnRequest {
            chat_id: &there,
            prompt: "run the tests",
            provider: "mock",
            model: "",
            routing: Some(crate::ui::WorkspaceRouting::Remote),
            medium_id: &medium,
            project_id: "remote",
            session_id: &there,
            workspace_dir: None,
            harness_id: Some("cli-a"),
            attachments: &[],
        })
        .expect("the remote route accepts the turn");
    rt.block_on(handle).expect("the turn settles");

    // What the daemon ran the turn on, as it announced it: the environment the
    // composer chose, and not the one the conversation's routing implies.
    let mut ran_on = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let goble_daemon_protocol::DaemonEvent::TraceStarted {
            session_id,
            medium_id,
            ..
        } = event
        {
            if session_id.0 == there {
                ran_on.push(medium_id.0);
            }
        }
    }
    assert_eq!(
        ran_on,
        vec!["local".to_string()],
        "the submitted turn's environment is the chosen one"
    );
}
