//! Re-attaching to a conversation's session instead of starting over.
//!
//! The run lives where its session does, so a client that wakes — a pane opened
//! on a conversation that already ran, or a connection that dropped and came
//! back — must rejoin what is still there rather than open a fresh turn or an
//! empty transcript. The machinery is the daemon's own reversibility, already on
//! the wire ([`goble_daemon_protocol::DaemonRequest::Replay`] and friends): the
//! session's recorded turns (the daemon's checkpoints) give the span to replay,
//! the daemon re-emits that span, and its snapshot says whether a turn is on the
//! session right now.
//!
//! Two rules this module keeps:
//!
//! * the transcript a client shows after a re-attach is the session's own
//!   recording ([`AttachedSession::messages`], folded from the turns the daemon
//!   recorded and the events they produced) — never this machine's rows about
//!   the conversation, which describe a run that may not be the session's;
//! * a conversation with no session says so ([`SessionAttach::NoSession`]);
//!   nothing here starts one.

use goble_core::harness::{ChatToolCall, ToolCallStatus};
use goble_daemon_protocol::DaemonEvent;
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_types::SessionId;
use goble_replay::TurnRecord;

use super::{ChatMessage, DesktopState};

/// A conversation's session, as a client sees it when it rejoins.
#[derive(Debug, Clone)]
pub struct AttachedSession {
    /// The session's own id, as its recording carries it. A re-attach never
    /// renames a session: a client that dropped and came back is handed the
    /// same id it left.
    pub session_id: String,
    /// The turn index the replayed span starts at — how much of the session the
    /// client already had (`0` on a fresh open).
    pub from_turn: usize,
    /// The turns the session has recorded now.
    pub turns: usize,
    /// Whether a turn is running on the session right now. A settled session
    /// keeps its recording, so it can still be re-attached to after the run.
    pub live: bool,
    /// The session's own transcript for turns `from_turn..`: each turn's prompt,
    /// the reply its events streamed, and the tool calls that turn made with the
    /// outcomes they recorded.
    ///
    /// This is what a pane draws after a re-attach. It is folded from the
    /// session's own recording, so it is what the session ran.
    pub messages: Vec<ChatMessage>,
    /// The session's recorded events for the same span, re-emitted in the daemon
    /// wire shape. The transcript above is the same span read as rows; this is
    /// the replay a client re-emits for a live stream.
    pub events: Vec<DaemonEvent>,
}

/// What a client got when it asked to rejoin a conversation's session.
#[derive(Debug, Clone)]
pub enum SessionAttach {
    /// The session is there: live, or settled with its recording kept.
    Attached(AttachedSession),
    /// The conversation has no session: nothing ran under it and no turn is on
    /// it now. The client must say so rather than start one.
    NoSession { chat_id: String },
}

impl SessionAttach {
    /// The session this outcome is about, if there is one.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Attached(session) => Some(&session.session_id),
            Self::NoSession { .. } => None,
        }
    }
}

impl DesktopState {
    /// Rejoin the session `chat_id`'s work runs on, replaying the turns this
    /// client has not seen.
    ///
    /// `from_turn` is how much of the session the client already has: `0` for a
    /// pane opening on the conversation for the first time, and the turn count
    /// the previous attach reported for one coming back after a drop — so a
    /// re-attach replays what was missed, not the whole session again.
    ///
    /// A conversation and the session under it share one id: a chat's turn runs
    /// on the daemon as `SessionId(<session_id>)`, and the app submits a pane's
    /// turn with that pane's conversation id ([`Self::run_chat_turn`]). That is
    /// what makes a re-attach land on the session the client left instead of on
    /// a new one.
    ///
    /// Nothing is started here. A conversation with no session and no turn on it
    /// comes back as [`SessionAttach::NoSession`], and no harness is invoked.
    pub fn attach_chat_session(&self, chat_id: &str, from_turn: usize) -> SessionAttach {
        let sid = SessionId::new(chat_id);
        let live = self
            .daemon
            .snapshot()
            .iter()
            .any(|record| record.session_id == sid);
        // The session's recording is its checkpoints: index `at` holds the
        // prefix `[0, at)`, so the last one holds every turn recorded so far
        // under the session's own id.
        let checkpoints = self.daemon.checkpoints(&sid).unwrap_or_default();
        let recorded = checkpoints
            .last()
            .map(|checkpoint| checkpoint.turns.clone())
            .unwrap_or_default();
        if !live && recorded.is_empty() {
            return SessionAttach::NoSession {
                chat_id: chat_id.to_string(),
            };
        }
        let session_id = checkpoints
            .last()
            .map(|checkpoint| checkpoint.session_id.clone())
            .unwrap_or_else(|| sid.clone());
        let turns = recorded.len();
        let from_turn = from_turn.min(turns);
        SessionAttach::Attached(AttachedSession {
            session_id: session_id.0.clone(),
            from_turn,
            turns,
            live,
            messages: transcript_rows(&session_id, &recorded[from_turn..]),
            // A replay past the end is empty rather than an error, so a client
            // that already holds every turn gets nothing new and keeps what it
            // has.
            events: self.daemon.replay(&sid, from_turn).unwrap_or_default(),
        })
    }
}

/// The transcript rows one span of a session's recorded turns makes: a prompt,
/// a reply, and the tool calls that turn made.
///
/// The rows are the session's own — the prompt is the goal the harness was
/// handed and the reply is the text its events streamed — with ids derived from
/// the session and the turn index, so a client that re-attaches to the same span
/// resolves the same rows rather than inventing new ones.
fn transcript_rows(session_id: &SessionId, turns: &[TurnRecord]) -> Vec<ChatMessage> {
    let mut rows: Vec<ChatMessage> = Vec::new();
    for turn in turns {
        let started = turn.started_at.to_rfc3339();
        rows.push(ChatMessage {
            id: format!("{}:turn-{}:user", session_id.0, turn.index),
            role: "user".to_string(),
            content: turn.turn.goal.clone(),
            tool_calls: None,
            created_at: started.clone(),
        });
        let calls = recorded_tool_calls(&turn.events);
        let reply = streamed_text(&turn.events);
        // A turn that streamed no prose and made no call (a cancelled one, say)
        // leaves no bubble behind; the prompt it was asked is already there.
        if reply.is_empty() && calls.is_empty() {
            continue;
        }
        rows.push(ChatMessage {
            id: format!("{}:turn-{}:assistant", session_id.0, turn.index),
            role: "assistant".to_string(),
            content: reply,
            tool_calls: (!calls.is_empty())
                .then(|| serde_json::to_string(&calls).unwrap_or_default()),
            created_at: turn
                .finished_at
                .map(|at| at.to_rfc3339())
                .unwrap_or(started),
        });
    }
    rows
}

/// The assistant prose a turn's events streamed, in order.
fn streamed_text(events: &[HarnessServerEvent]) -> String {
    let mut text = String::new();
    for event in events {
        if let HarnessServerEvent::AssistantDelta { delta, .. } = event {
            text.push_str(delta);
        }
    }
    text
}

/// The tool calls a turn recorded, each carrying the lifecycle state its own
/// events left it in — `Finished` with the result the call produced, or `Error`
/// with what went wrong. A call that started and never finished stays `Running`,
/// which is what the session was left holding.
fn recorded_tool_calls(events: &[HarnessServerEvent]) -> Vec<ChatToolCall> {
    let mut calls: Vec<ChatToolCall> = Vec::new();
    for event in events {
        match event {
            HarnessServerEvent::ToolCallStarted {
                id,
                name,
                arguments,
                ..
            } => calls.push(ChatToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
                status: ToolCallStatus::Running,
                result: None,
            }),
            HarnessServerEvent::ToolCallFinished { id, result, .. } => {
                if let Some(call) = calls.iter_mut().find(|call| &call.id == id) {
                    call.status = ToolCallStatus::Finished;
                    call.result = Some(result.clone());
                }
            }
            HarnessServerEvent::ToolCallError { id, message, .. } => {
                if let Some(call) = calls.iter_mut().find(|call| &call.id == id) {
                    call.status = ToolCallStatus::Error;
                    call.result = Some(message.clone());
                }
            }
            _ => {}
        }
    }
    calls
}
