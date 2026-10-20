//! The app side of the control protocol.
//!
//! The control server owns a background thread, but every UI verb must run on
//! the winit thread that owns [`UiState`]. The server's handler therefore
//! forwards non-PTY commands over a channel with a reply channel; the root
//! view drains that channel once per frame (on the winit thread) and answers.
//! If the window closes, the receiver drops and a waiting handler gets a
//! structured "the app closed" error instead of hanging.
//!
//! `pty.*` never touches the UI thread: the handler talks to the F1 PTY daemon
//! directly, on the same background thread.

#![cfg(unix)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use goble_control::{
    AgentStateInfo, Command, ControlError, ErrorCode, Handler, InboxInfo, LayoutInfo, LayoutKind,
    LayoutNode, PaneInfo, PaneKind as ControlPaneKind, Payload, PromptResult, PtySessionInfo,
    SendResult, Server, SpaceInfo, SplitAxis,
};
use goble_desktop_service::DesktopState;
use goble_pty::protocol::Request as PtyRequest;
use goble_pty::{Client as PtyClient, ServerMessage as PtyMessage};

use crate::media::MediaState;
use crate::state::{collect_leaf_pane_ids, default_pane_path, PaneSession, UiState};
use crate::ui::{LayoutKind as UiLayoutKind, Pane, PaneKind, SplitDir};

/// How long a handler waits for the winit thread to answer. A busy turn or a
/// slow frame is still answered; a closed window is not, and that case takes
/// the disconnected path immediately.
const UI_REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// The timeout a control request waits for the PTY daemon.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(10);

/// One UI verb waiting for the winit thread.
#[derive(Debug)]
pub struct PendingCommand {
    pub command: Command,
    reply_to: SyncSender<Result<Payload, ControlError>>,
}

/// The handler the control server owns. `pty.*` is answered here; everything
/// else is forwarded to the root view.
struct AppControlHandler {
    ui: Sender<PendingCommand>,
}

impl Handler for AppControlHandler {
    fn handle(&self, command: Command) -> Result<Payload, ControlError> {
        if command.is_pty() {
            return handle_pty(command);
        }
        let (reply, wait) = mpsc::sync_channel(1);
        self.ui
            .send(PendingCommand { command, reply_to: reply })
            .map_err(|_| ControlError::unavailable("the Goble window closed before the request arrived"))?;
        match wait.recv_timeout(UI_REPLY_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ControlError::unavailable(
                "the Goble window did not answer the control request in time",
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ControlError::unavailable(
                "the Goble window closed before the request was handled",
            )),
        }
    }
}

/// A running control server plus the channel the root view drains.
pub struct ControlBridge {
    receiver: Receiver<PendingCommand>,
    server: Server<AppControlHandler>,
}

impl std::fmt::Debug for ControlBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlBridge")
            .field("socket", &self.server.socket_path())
            .finish_non_exhaustive()
    }
}

impl ControlBridge {
    /// The socket the server bound.
    pub fn socket_path(&self) -> &std::path::Path {
        self.server.socket_path()
    }

    /// Take one UI verb, if one is waiting.
    pub fn try_recv(&self) -> Option<PendingCommand> {
        self.receiver.try_recv().ok()
    }
}

impl PendingCommand {
    /// Answer the waiting handler.
    pub fn reply(self, result: Result<Payload, ControlError>) {
        let _ = self.reply_to.send(result);
    }
}

/// Serve the default control socket (`$GOBLE_CONTROL_SOCKET`, else
/// `$HOME/.goble/control.sock`).
pub fn start_default() -> Result<ControlBridge, ControlError> {
    start(goble_control::resolve_socket_path())
}

/// Serve `socket`. Fails with a structured error when another app instance is
/// already listening; the caller may keep running without the control surface.
pub fn start(socket: impl AsRef<std::path::Path>) -> Result<ControlBridge, ControlError> {
    let (sender, receiver) = mpsc::channel();
    let server = Server::start(socket, AppControlHandler { ui: sender })?;
    Ok(ControlBridge { receiver, server })
}

/// Run one UI verb on the winit thread. Every failure is a [`ControlError`],
/// so the caller never has to distinguish a panic from a bad request.
pub fn handle_ui_command(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<RefCell<MediaState>>,
    command: Command,
) -> Result<Payload, ControlError> {
    match command {
        Command::PanesList => Ok(Payload::Panes {
            panes: all_panes(state),
        }),
        Command::PanesNew {
            space_id,
            cwd,
            kind,
        } => {
            let space_id = space_id.unwrap_or(state.active_space);
            let target = target_pane_in_space(state, space_id)?;
            let kind = kind.unwrap_or(ControlPaneKind::Terminal);
            split_pane(state, desktop, target, SplitAxis::Horizontal, Some(kind), cwd)
        }
        Command::PanesSplit { pane_id, axis } => {
            split_pane(state, desktop, pane_id, axis, None, None)
        }
        Command::PanesClose { pane_id } => close_pane(state, desktop, pane_id),
        Command::PanesFocus { pane_id } => {
            let space_id = space_of_pane(state, pane_id)?;
            state.current_tab = crate::ui::AppTab::Chat;
            state.active_space = space_id;
            state.focus_pane(pane_id);
            state.sync_active_view();
            pane_info(state, space_id, pane_id)
                .map(|pane| Payload::Pane { pane })
        }
        Command::PanesSend {
            pane_id,
            text,
            enter,
        } => send_to_pane(state, pane_id, &text, enter),
        Command::PanesResize {
            pane_id,
            cols,
            rows,
        } => resize_pane(state, pane_id, cols, rows),
        Command::SpacesList => Ok(Payload::Spaces {
            spaces: (0..state.spaces.len())
                .filter_map(|index| space_info(state, index))
                .collect(),
        }),
        Command::SpacesSwitch { space_id } => switch_space(state, space_id),
        Command::AgentsState { pane_id } => agents_state(state, pane_id),
        Command::AgentsPrompt { pane_id, text } => {
            space_of_pane(state, pane_id)?;
            let started = crate::actions::prompt::send_agent_prompt(
                state,
                desktop,
                media,
                &text,
                pane_id,
            );
            Ok(Payload::Prompt {
                prompt: PromptResult { pane_id, started },
            })
        }
        Command::InboxList => Ok(Payload::Inbox {
            items: state.inbox.iter().map(inbox_info).collect(),
        }),
        Command::InboxAck { id, all } => ack_inbox(state, id, all),
        Command::LayoutGet { space_id } => layout_get(state, space_id),
        Command::LayoutSet { space_id, kind } => layout_set(state, desktop, space_id, kind),
        Command::PtyList
        | Command::PtyAttach { .. }
        | Command::PtyDetach { .. }
        | Command::PtyKill { .. } => Err(ControlError::internal(
            "PTY verbs are served off the UI thread",
        )),
    }
}

fn all_panes(state: &UiState) -> Vec<PaneInfo> {
    let mut panes = Vec::new();
    for (space_id, space) in state.spaces.iter().enumerate() {
        let mut ids = Vec::new();
        collect_leaf_pane_ids(&space.root, &mut ids);
        for pane_id in ids {
            if let Ok(info) = pane_info(state, space_id, pane_id) {
                panes.push(info);
            }
        }
    }
    panes
}

fn pane_info(state: &UiState, space_id: usize, pane_id: u64) -> Result<PaneInfo, ControlError> {
    let kind = state
        .spaces
        .get(space_id)
        .and_then(|space| space.leaf_kind(pane_id))
        .ok_or_else(|| ControlError::missing_pane(pane_id))?;
    Ok(PaneInfo {
        pane_id,
        space_id,
        kind: kind_name(kind),
        title: state.pane_title(pane_id),
        cwd: state
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.path.clone())
            .filter(|path| !path.trim().is_empty()),
        focused: state.active_space == space_id && state.active_pane_id == pane_id,
        active_space: state.active_space == space_id,
    })
}

fn space_info(state: &UiState, space_id: usize) -> Option<SpaceInfo> {
    let space = state.spaces.get(space_id)?;
    Some(SpaceInfo {
        space_id,
        name: state.space_label(space_id),
        medium: space.medium.clone(),
        pane_count: space.root.leaf_count(),
        layout: space_layout_kind(space),
        active: state.active_space == space_id,
    })
}

fn kind_name(kind: &PaneKind) -> String {
    match kind {
        PaneKind::Chat => "chat",
        PaneKind::Terminal => "terminal",
        PaneKind::Worker => "worker",
        PaneKind::File { .. } => "file",
        PaneKind::Settings { .. } => "settings",
    }
    .to_string()
}

fn control_kind(kind: ControlPaneKind) -> PaneKind {
    match kind {
        ControlPaneKind::Chat => PaneKind::Chat,
        ControlPaneKind::Terminal => PaneKind::Terminal,
    }
}

fn space_of_pane(state: &UiState, pane_id: u64) -> Result<usize, ControlError> {
    state
        .spaces
        .iter()
        .position(|space| space.root.contains_leaf(pane_id))
        .ok_or_else(|| ControlError::missing_pane(pane_id))
}

fn target_pane_in_space(state: &UiState, space_id: usize) -> Result<u64, ControlError> {
    let space = state
        .spaces
        .get(space_id)
        .ok_or_else(|| ControlError::missing_space(space_id))?;
    if state.active_space == space_id && space.root.contains_leaf(state.active_pane_id) {
        Ok(state.active_pane_id)
    } else {
        Ok(space.root.first_leaf_id())
    }
}

fn split_pane(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    pane_id: u64,
    axis: SplitAxis,
    kind: Option<ControlPaneKind>,
    cwd: Option<String>,
) -> Result<Payload, ControlError> {
    let space_id = space_of_pane(state, pane_id)?;
    let dir = match axis {
        SplitAxis::Horizontal => SplitDir::Horizontal,
        SplitAxis::Vertical => SplitDir::Vertical,
    };
    let inherited = state
        .pane_sessions
        .get(&pane_id)
        .map(|session| session.path.clone())
        .unwrap_or_else(|| default_pane_path("", desktop.map(|d| d.as_ref())));
    let target_kind = state.spaces[space_id]
        .leaf_kind(pane_id)
        .cloned()
        .ok_or_else(|| ControlError::missing_pane(pane_id))?;

    let mut next_id = state.next_pane_id;
    let new_id = match kind {
        Some(kind) => state.spaces[space_id].split_with_kind(
            pane_id,
            dir,
            &mut next_id,
            control_kind(kind),
        ),
        None if matches!(target_kind, PaneKind::Chat | PaneKind::Terminal) => {
            state.spaces[space_id].split(pane_id, dir, &mut next_id)
        }
        None => state.spaces[space_id].split_with_kind(
            pane_id,
            dir,
            &mut next_id,
            PaneKind::Terminal,
        ),
    };
    let Some(new_id) = new_id else {
        return Err(ControlError::new(
            ErrorCode::Unsupported,
            format!("pane {pane_id} could not be split"),
        ));
    };
    state.next_pane_id = next_id;
    state.active_space = space_id;
    state.current_tab = crate::ui::AppTab::Chat;
    state.focus_pane(new_id);
    state
        .pane_hover
        .entry(new_id)
        .or_insert_with(|| Rc::new(RefCell::new(false)));
    state.pane_sessions.insert(
        new_id,
        PaneSession {
            conversation_id: String::new(),
            draft: String::new(),
            path: cwd.unwrap_or(inherited),
            daemon_session_id: String::new(),
        },
    );
    if matches!(
        state.spaces[space_id].leaf_kind(new_id),
        Some(PaneKind::Chat)
    ) {
        state.bind_pane_new_conversation(new_id, desktop.map(|d| d.as_ref()));
    }
    state.ensure_pane_controls();
    state.sync_active_view();
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
    pane_info(state, space_id, new_id).map(|pane| Payload::Pane { pane })
}

fn close_pane(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    pane_id: u64,
) -> Result<Payload, ControlError> {
    let space_id = space_of_pane(state, pane_id)?;
    let next = state.spaces[space_id].close(pane_id);
    let Some(next) = next else {
        return Err(ControlError::new(
            ErrorCode::Unsupported,
            format!("pane {pane_id} is the only pane in space {space_id}"),
        ));
    };
    state.forget_pane(pane_id);
    if state.maximized_pane == Some(pane_id) {
        state.maximized_pane = None;
    }
    state.active_space = space_id;
    state.focus_pane(next);
    state.sync_active_view();
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
    Ok(Payload::Closed { pane_id })
}

fn send_to_pane(
    state: &mut UiState,
    pane_id: u64,
    text: &str,
    enter: bool,
) -> Result<Payload, ControlError> {
    let space_id = space_of_pane(state, pane_id)?;
    if matches!(
        state.spaces[space_id].leaf_kind(pane_id),
        Some(PaneKind::Worker | PaneKind::File { .. } | PaneKind::Settings { .. })
    ) {
        return Err(ControlError::new(
            ErrorCode::Unsupported,
            format!("pane {pane_id} has no local shell"),
        ));
    }
    let cwd = state
        .pane_sessions
        .get(&pane_id)
        .map(|session| session.path.clone())
        .unwrap_or_default();
    let terminal = Rc::clone(&state.terminal);
    let mut terminal = terminal.borrow_mut();
    terminal.ensure_session(pane_id, &cwd);
    let Some(session) = terminal.sessions.get_mut(&pane_id) else {
        return Err(ControlError::internal(format!(
            "pane {pane_id} has no terminal session"
        )));
    };
    if !session.is_alive() {
        return Err(ControlError::new(
            ErrorCode::Unsupported,
            format!("pane {pane_id}'s shell is not running"),
        ));
    }
    let mut bytes = text.as_bytes().to_vec();
    if enter {
        bytes.push(b'\r');
    }
    session.write(&bytes);
    Ok(Payload::Sent {
        send: SendResult {
            pane_id,
            bytes: bytes.len(),
            entered: enter,
        },
    })
}

fn resize_pane(
    state: &mut UiState,
    pane_id: u64,
    cols: u16,
    rows: u16,
) -> Result<Payload, ControlError> {
    if cols == 0 || rows == 0 {
        return Err(ControlError::invalid_request("cols and rows must be greater than zero"));
    }
    let space_id = space_of_pane(state, pane_id)?;
    if matches!(
        state.spaces[space_id].leaf_kind(pane_id),
        Some(PaneKind::Worker | PaneKind::File { .. } | PaneKind::Settings { .. })
    ) {
        return Err(ControlError::new(
            ErrorCode::Unsupported,
            format!("pane {pane_id} has no terminal to resize"),
        ));
    }
    let cwd = state
        .pane_sessions
        .get(&pane_id)
        .map(|session| session.path.clone())
        .unwrap_or_default();
    let terminal = Rc::clone(&state.terminal);
    let mut terminal = terminal.borrow_mut();
    terminal.ensure_session(pane_id, &cwd);
    if !terminal.resize(pane_id, cols, rows) {
        return Err(ControlError::internal(format!(
            "pane {pane_id} has no terminal session"
        )));
    }
    Ok(Payload::Ok)
}

fn switch_space(state: &mut UiState, space_id: usize) -> Result<Payload, ControlError> {
    let first = state
        .spaces
        .get(space_id)
        .map(|space| space.root.first_leaf_id())
        .ok_or_else(|| ControlError::missing_space(space_id))?;
    state.current_tab = crate::ui::AppTab::Chat;
    state.active_space = space_id;
    state.focus_pane(first);
    state.sync_active_view();
    space_info(state, space_id)
        .map(|space| Payload::Space { space })
        .ok_or_else(|| ControlError::missing_space(space_id))
}

fn agents_state(
    state: &UiState,
    pane_id: Option<u64>,
) -> Result<Payload, ControlError> {
    let ids: Vec<u64> = match pane_id {
        Some(pane_id) => {
            space_of_pane(state, pane_id)?;
            vec![pane_id]
        }
        None => all_panes(state).into_iter().map(|pane| pane.pane_id).collect(),
    };
    let agents = ids
        .into_iter()
        .map(|pane_id| AgentStateInfo {
            pane_id,
            state: agent_state_name(state.pane_agent_state(pane_id)).to_string(),
            busy: state
                .pane_runtime
                .get(&pane_id)
                .map(|runtime| runtime.busy)
                .unwrap_or(false),
            conversation_id: state.pane_conversation_id(pane_id),
        })
        .collect();
    Ok(Payload::AgentStates { agents })
}

fn agent_state_name(state: crate::state::PaneAgentState) -> &'static str {
    use crate::state::PaneAgentState;
    match state {
        PaneAgentState::Unknown => "unknown",
        PaneAgentState::Idle => "idle",
        PaneAgentState::Working => "working",
        PaneAgentState::NeedsInput => "needs_input",
        PaneAgentState::Done => "done",
        PaneAgentState::Errored => "errored",
    }
}

fn inbox_info(item: &crate::state::InboxItem) -> InboxInfo {
    InboxInfo {
        id: item.id.clone(),
        pane_id: item.pane_id,
        space_id: item.space_id,
        conversation_id: item.conversation_id.clone(),
        kind: item.kind.label().to_ascii_lowercase(),
        title: item.title.clone(),
        detail: item.detail.clone(),
        created_at: item.created_at.to_rfc3339(),
        acknowledged: item.acknowledged,
    }
}

fn ack_inbox(
    state: &mut UiState,
    id: Option<String>,
    all: bool,
) -> Result<Payload, ControlError> {
    if all {
        let mut acknowledged = 0;
        for item in &mut state.inbox {
            if !item.acknowledged {
                item.acknowledged = true;
                acknowledged += 1;
            }
        }
        return Ok(Payload::Acknowledged { acknowledged });
    }
    let Some(id) = id else {
        return Err(ControlError::invalid_request(
            "inbox.ack needs an `id` or `all: true`",
        ));
    };
    let Some(item) = state.inbox.iter_mut().find(|item| item.id == id) else {
        return Err(ControlError::new(
            ErrorCode::MissingItem,
            format!("inbox item `{id}` is not in the inbox"),
        ));
    };
    let acknowledged = usize::from(!item.acknowledged);
    item.acknowledged = true;
    Ok(Payload::Acknowledged { acknowledged })
}

fn layout_get(state: &UiState, space_id: usize) -> Result<Payload, ControlError> {
    let space = state
        .spaces
        .get(space_id)
        .ok_or_else(|| ControlError::missing_space(space_id))?;
    let active_pane_id = if state.active_space == space_id
        && space.root.contains_leaf(state.active_pane_id)
    {
        state.active_pane_id
    } else {
        space.root.first_leaf_id()
    };
    Ok(Payload::Layout {
        layout: LayoutInfo {
            space_id,
            kind: space_layout_kind(space),
            active_pane_id,
            root: layout_node(&space.root),
        },
    })
}

fn layout_kind(pane: &Pane) -> LayoutKind {
    match pane {
        Pane::Leaf { .. } => LayoutKind::Single,
        Pane::Split {
            dir: SplitDir::Horizontal,
            ..
        } => LayoutKind::Horizontal,
        Pane::Split {
            dir: SplitDir::Vertical,
            ..
        } => LayoutKind::Vertical,
    }
}

/// The layout a space reports. A BSP space keeps reporting its split
/// orientation (`single`, `horizontal`, `vertical`), so a client that predates
/// the window-manager layouts reads the same tree it always did; the two newer
/// layouts are named as themselves.
fn space_layout_kind(space: &crate::ui::Space) -> LayoutKind {
    match space.layout {
        UiLayoutKind::Bsp => layout_kind(&space.root),
        UiLayoutKind::MasterStack => LayoutKind::MasterStack,
        UiLayoutKind::Floating => LayoutKind::Floating,
        UiLayoutKind::Carousel => LayoutKind::Carousel,
    }
}

fn layout_node(pane: &Pane) -> LayoutNode {
    match pane {
        Pane::Leaf { id, kind } => LayoutNode::Leaf {
            pane_id: *id,
            kind: kind_name(kind),
        },
        Pane::Split {
            id,
            dir,
            ratio,
            first,
            second,
        } => LayoutNode::Split {
            split_id: *id,
            axis: match dir {
                SplitDir::Horizontal => SplitAxis::Horizontal,
                SplitDir::Vertical => SplitAxis::Vertical,
            },
            ratio: *ratio,
            first: Box::new(layout_node(first)),
            second: Box::new(layout_node(second)),
        },
    }
}

fn layout_set(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    space_id: usize,
    kind: LayoutKind,
) -> Result<Payload, ControlError> {
    let active_pane_id = {
        let space = state
            .spaces
            .get(space_id)
            .ok_or_else(|| ControlError::missing_space(space_id))?;
        if state.active_space == space_id && space.root.contains_leaf(state.active_pane_id) {
            state.active_pane_id
        } else {
            space.root.first_leaf_id()
        }
    };
    match kind {
        LayoutKind::Single => {
            let mut ids = Vec::new();
            collect_leaf_pane_ids(&state.spaces[space_id].root, &mut ids);
            let keep = active_pane_id;
            let keep_kind = state.spaces[space_id]
                .leaf_kind(keep)
                .cloned()
                .ok_or_else(|| ControlError::missing_pane(keep))?;
            for pane_id in ids.into_iter().filter(|id| *id != keep) {
                if state.maximized_pane == Some(pane_id) {
                    state.maximized_pane = None;
                }
                state.forget_pane(pane_id);
            }
            let space = &mut state.spaces[space_id];
            space.root = Pane::Leaf {
                id: keep,
                kind: keep_kind,
            };
            space.set_layout(UiLayoutKind::Bsp);
            state.active_space = space_id;
            state.focus_pane(keep);
        }
        LayoutKind::Horizontal | LayoutKind::Vertical => {
            let dir = if matches!(kind, LayoutKind::Horizontal) {
                SplitDir::Horizontal
            } else {
                SplitDir::Vertical
            };
            let space = &mut state.spaces[space_id];
            space.set_layout_direction(dir);
            space.set_layout(UiLayoutKind::Bsp);
        }
        LayoutKind::Bsp => {
            state.spaces[space_id].set_layout(UiLayoutKind::Bsp);
        }
        LayoutKind::MasterStack => {
            state.spaces[space_id].set_layout(UiLayoutKind::MasterStack);
        }
        LayoutKind::Floating => {
            state.spaces[space_id].set_layout(UiLayoutKind::Floating);
        }
        LayoutKind::Carousel => {
            state.spaces[space_id].set_layout(UiLayoutKind::Carousel);
        }
    }
    if state.active_space == space_id
        && !matches!(state.spaces[space_id].layout, UiLayoutKind::Floating)
    {
        state.floating_drag.borrow_mut().take();
    }
    state.current_tab = crate::ui::AppTab::Chat;
    state.sync_active_view();
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
    layout_get(state, space_id)
}

fn handle_pty(command: Command) -> Result<Payload, ControlError> {
    match command {
        Command::PtyList => {
            let mut client = pty_client()?;
            let id = client.next_id();
            match pty_call(&mut client, PtyRequest::List { id })? {
                PtyMessage::Sessions { sessions, .. } => Ok(Payload::PtySessions {
                    sessions: sessions.iter().map(pty_session).collect(),
                }),
                other => Err(unexpected_pty("list", other)),
            }
        }
        Command::PtyAttach { session_id } => {
            let mut client = pty_client()?;
            let id = client.next_id();
            match pty_call(
                &mut client,
                PtyRequest::Hello {
                    id,
                    client_id: "goble-control".to_string(),
                },
            )? {
                PtyMessage::Ok { .. } => {}
                other => return Err(unexpected_pty("hello", other)),
            }
            let id = client.next_id();
            match pty_call(
                &mut client,
                PtyRequest::Attach {
                    id,
                    session_id,
                    from_seq: 0,
                },
            )? {
                PtyMessage::Ok {
                    session: Some(info),
                    ..
                } => Ok(Payload::PtySession {
                    session: pty_session(&info),
                }),
                PtyMessage::Ok { session: None, .. } => Err(ControlError::new(
                    ErrorCode::PtyUnavailable,
                    "the PTY daemon attached without reporting a session",
                )),
                other => Err(unexpected_pty("attach", other)),
            }
        }
        Command::PtyDetach { session_id } => {
            let mut client = pty_client()?;
            let id = client.next_id();
            match pty_call(&mut client, PtyRequest::Detach { id, session_id })? {
                PtyMessage::Ok { .. } => Ok(Payload::Ok),
                other => Err(unexpected_pty("detach", other)),
            }
        }
        Command::PtyKill { session_id } => {
            let mut client = pty_client()?;
            let id = client.next_id();
            match pty_call(&mut client, PtyRequest::Kill { id, session_id })? {
                PtyMessage::Ok { .. } => Ok(Payload::Ok),
                other => Err(unexpected_pty("kill", other)),
            }
        }
        other => Err(ControlError::internal(format!(
            "{} is not a PTY verb",
            other.verb()
        ))),
    }
}

fn pty_client() -> Result<PtyClient, ControlError> {
    let socket = goble_pty::resolve_socket_path();
    let client = PtyClient::connect_default().map_err(|error| {
        ControlError::new(
            ErrorCode::PtyUnavailable,
            format!(
                "could not connect to the PTY daemon at {}: {error}",
                socket.display()
            ),
        )
    })?;
    client
        .set_read_timeout(Some(DAEMON_TIMEOUT))
        .map_err(|error| {
            ControlError::new(
                ErrorCode::PtyUnavailable,
                format!("could not set the PTY daemon timeout: {error}"),
            )
        })?;
    Ok(client)
}

fn pty_call(
    client: &mut PtyClient,
    request: PtyRequest,
) -> Result<PtyMessage, ControlError> {
    match client.request(request) {
        Ok(PtyMessage::Error { message, .. }) => Err(ControlError::new(
            ErrorCode::PtyUnavailable,
            message,
        )),
        Ok(message) => Ok(message),
        Err(error) => Err(ControlError::new(
            ErrorCode::PtyUnavailable,
            format!("the PTY daemon request failed: {error}"),
        )),
    }
}

fn pty_session(info: &goble_pty::SessionInfo) -> PtySessionInfo {
    PtySessionInfo {
        session_id: info.session_id.clone(),
        pane_id: info.pane_id.clone(),
        alive: info.alive,
        cols: info.cols,
        rows: info.rows,
    }
}

fn unexpected_pty(verb: &str, message: PtyMessage) -> ControlError {
    ControlError::new(
        ErrorCode::PtyUnavailable,
        format!("the PTY daemon answered {verb} with {message:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> UiState {
        UiState::mock()
    }

    fn media() -> Rc<RefCell<MediaState>> {
        Rc::new(RefCell::new(MediaState::mock()))
    }

    #[test]
    fn panes_list_reports_the_open_panes() {
        let mut state = state();
        let payload = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::PanesList,
        )
        .unwrap();
        match payload {
            Payload::Panes { panes } => {
                assert_eq!(panes.len(), 1);
                assert_eq!(panes[0].pane_id, 1);
                assert_eq!(panes[0].kind, "chat");
                assert!(panes[0].focused);
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn split_adds_a_pane_and_a_missing_pane_is_structured() {
        let mut state = state();
        let payload = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::PanesSplit {
                pane_id: 1,
                axis: SplitAxis::Vertical,
            },
        )
        .unwrap();
        let new_id = match payload {
            Payload::Pane { pane } => {
                assert!(pane.pane_id > 1);
                pane.pane_id
            }
            other => panic!("unexpected payload: {other:?}"),
        };
        assert!(state.spaces[0].root.contains_leaf(new_id));

        let error = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::PanesFocus { pane_id: 999 },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::MissingPane);
    }

    #[test]
    fn closing_the_only_pane_is_refused_without_touching_it() {
        let mut state = state();
        let error = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::PanesClose { pane_id: 1 },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(state.spaces[0].root.contains_leaf(1));
    }

    #[test]
    fn spaces_switch_and_layout_set_reach_the_state() {
        let mut state = state();
        let second = match handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::PanesNew {
                space_id: Some(0),
                cwd: Some("/tmp".into()),
                kind: Some(ControlPaneKind::Terminal),
            },
        )
        .unwrap()
        {
            Payload::Pane { pane } => pane.pane_id,
            other => panic!("unexpected payload: {other:?}"),
        };
        assert!(second > 1);

        match handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::LayoutGet { space_id: 0 },
        )
        .unwrap()
        {
            Payload::Layout { layout } => {
                assert_eq!(layout.kind, LayoutKind::Horizontal);
                assert!(matches!(layout.root, LayoutNode::Split { .. }));
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::LayoutSet {
                space_id: 0,
                kind: LayoutKind::Single,
            },
        )
        .unwrap();
        assert_eq!(state.spaces[0].root.leaf_count(), 1);
    }

    #[test]
    fn layout_set_selects_the_window_manager_layouts() {
        let mut state = state();
        handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::PanesNew {
                space_id: Some(0),
                cwd: Some("/tmp".into()),
                kind: Some(ControlPaneKind::Terminal),
            },
        )
        .unwrap();

        match handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::LayoutSet {
                space_id: 0,
                kind: LayoutKind::MasterStack,
            },
        )
        .unwrap()
        {
            Payload::Layout { layout } => assert_eq!(layout.kind, LayoutKind::MasterStack),
            other => panic!("unexpected payload: {other:?}"),
        }
        assert_eq!(
            state.spaces[0].layout,
            crate::ui::LayoutKind::MasterStack
        );

        match handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::LayoutSet {
                space_id: 0,
                kind: LayoutKind::Floating,
            },
        )
        .unwrap()
        {
            Payload::Layout { layout } => assert_eq!(layout.kind, LayoutKind::Floating),
            other => panic!("unexpected payload: {other:?}"),
        }
        assert_eq!(state.spaces[0].layout, crate::ui::LayoutKind::Floating);
        assert_eq!(
            state.spaces[0].floating.len(),
            state.spaces[0].root.leaf_count(),
            "every leaf has a floating rect"
        );

        // The legacy orientation verbs still orient the BSP tree, and they
        // leave the floating layout for it.
        match handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::LayoutSet {
                space_id: 0,
                kind: LayoutKind::Horizontal,
            },
        )
        .unwrap()
        {
            Payload::Layout { layout } => assert_eq!(layout.kind, LayoutKind::Horizontal),
            other => panic!("unexpected payload: {other:?}"),
        }
        assert_eq!(state.spaces[0].layout, crate::ui::LayoutKind::Bsp);
    }

    #[test]
    fn inbox_ack_marks_one_or_all() {
        let mut state = state();
        state.inbox = vec![
            crate::state::InboxItem::new(
                "i1".to_string(),
                1,
                0,
                "c1".to_string(),
                crate::state::InboxKind::Done,
                "done".to_string(),
                "detail".to_string(),
            ),
            crate::state::InboxItem::new(
                "i2".to_string(),
                1,
                0,
                "c1".to_string(),
                crate::state::InboxKind::Error,
                "failed".to_string(),
                "detail".to_string(),
            ),
        ];
        let payload = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::InboxAck {
                id: Some("i1".into()),
                all: false,
            },
        )
        .unwrap();
        assert!(matches!(payload, Payload::Acknowledged { acknowledged: 1 }));
        assert!(state.inbox[0].acknowledged);
        assert!(!state.inbox[1].acknowledged);

        let payload = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::InboxAck {
                id: None,
                all: true,
            },
        )
        .unwrap();
        assert!(matches!(payload, Payload::Acknowledged { acknowledged: 1 }));
        assert!(state.inbox.iter().all(|item| item.acknowledged));

        let error = handle_ui_command(
            &mut state,
            None,
            &media(),
            Command::InboxAck {
                id: Some("nope".into()),
                all: false,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::MissingItem);
    }

    #[test]
    fn agents_state_reports_every_pane() {
        let state = state();
        match agents_state(&state, None).unwrap() {
            Payload::AgentStates { agents } => {
                assert_eq!(agents.len(), 1);
                assert_eq!(agents[0].pane_id, 1);
                assert_eq!(agents[0].state, "unknown");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn a_closed_window_answers_with_unavailable() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let handler = AppControlHandler { ui: sender };
        let error = handler.handle(Command::PanesList).unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(error.message.contains("closed"));
    }

    #[test]
    fn a_ui_verb_crosses_the_threads_and_returns_its_answer() {
        let (sender, receiver) = mpsc::channel();
        let handler = AppControlHandler { ui: sender };
        let worker = std::thread::spawn(move || handler.handle(Command::PanesList));

        let pending = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let command = pending.command.clone();
        let mut state = state();
        let result = handle_ui_command(&mut state, None, &media(), command);
        pending.reply(result);

        match worker.join().unwrap().unwrap() {
            Payload::Panes { panes } => assert_eq!(panes.len(), 1),
            other => panic!("unexpected payload: {other:?}"),
        }
    }
}
