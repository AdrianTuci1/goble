//! Transport-agnostic session state machine.
//!
//! One session owns one pty pane and one WebSocket. It does not know about the
//! DOM or `web_sys`: callers feed it clock readings, socket lifecycle events
//! and relayed frames, and it returns [`Plan`]s describing what to send and
//! when to retry. That makes reconnect/backoff fully testable with a fake
//! clock.
//!
//! A worker pane is owned by the socket that opened it, so a reconnect always
//! opens a **fresh** pane id; the previous shell died with the socket.

use crate::protocol::{frame_pane, output_bytes, ServerFrame};

/// First retry delay.
pub const BASE_BACKOFF_MS: u64 = 500;
/// Retry delay ceiling.
pub const MAX_BACKOFF_MS: u64 = 8_000;

/// Where a session is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Socket is opening or the pty is being created.
    Connecting,
    /// WebSocket is open; `pty_opened` has not arrived yet.
    Open,
    /// `pty_opened` arrived; input and output flow.
    Live,
    /// Socket or pty is gone. May be waiting for a retry.
    Closed,
    /// The worker rejected the pty or the socket failed terminally.
    Failed,
}

/// An action the transport/DOM must perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    OpenPty {
        pane_id: String,
        cwd: String,
        cols: u16,
        rows: u16,
    },
    Resize {
        pane_id: String,
        cols: u16,
        rows: u16,
    },
    Input {
        pane_id: String,
        bytes: Vec<u8>,
    },
    Close {
        pane_id: String,
    },
    /// Schedule a retry after `ms`. No socket work is implied yet.
    ReconnectIn {
        ms: u64,
    },
    /// A retry is starting with a fresh pane; the UI writes a separator.
    Reconnected {
        pane_id: String,
    },
    Ping,
}

/// Something the session learned from a worker frame, for the UI/terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Opened,
    Output(Vec<u8>),
    Exited(Option<i32>),
    Failed(String),
    Pong,
}

/// Retry delay for the `attempt`-th consecutive failure (`0` is the first).
#[must_use]
pub fn backoff_ms(attempt: u32) -> u64 {
    BASE_BACKOFF_MS
        .saturating_mul(1u64 << attempt.min(6))
        .min(MAX_BACKOFF_MS)
}

/// One terminal session.
#[derive(Debug)]
pub struct Session {
    worker_id: String,
    tag: String,
    cwd: String,
    cols: u16,
    rows: u16,
    state: SessionState,
    pane_id: Option<String>,
    generation: u32,
    backoff_attempt: u32,
    reconnect_at: Option<u64>,
    closed_by_user: bool,
    live_once: bool,
    reconnect_count: u32,
}

impl Session {
    #[must_use]
    pub fn new(
        worker_id: impl Into<String>,
        tag: impl Into<String>,
        cwd: impl Into<String>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self {
            worker_id: worker_id.into(),
            tag: tag.into(),
            cwd: cwd.into(),
            cols: cols.max(1),
            rows: rows.max(1),
            state: SessionState::Closed,
            pane_id: None,
            generation: 0,
            backoff_attempt: 0,
            reconnect_at: None,
            closed_by_user: false,
            live_once: false,
            reconnect_count: 0,
        }
    }

    #[must_use]
    pub fn state(&self) -> SessionState {
        self.state
    }

    #[must_use]
    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    #[must_use]
    pub fn pane_id(&self) -> Option<&str> {
        self.pane_id.as_deref()
    }

    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    #[must_use]
    pub fn live_once(&self) -> bool {
        self.live_once
    }

    #[must_use]
    pub fn reconnect_count(&self) -> u32 {
        self.reconnect_count
    }

    /// Deadline for the pending retry, if any.
    #[must_use]
    pub fn reconnect_at(&self) -> Option<u64> {
        self.reconnect_at
    }

    /// Begin the first connection: one fresh pane and an `open_pty`.
    pub fn start(&mut self) -> Vec<Plan> {
        self.closed_by_user = false;
        self.reconnect_at = None;
        self.backoff_attempt = 0;
        self.state = SessionState::Connecting;
        let pane_id = self.fresh_pane();
        vec![self.open_plan(pane_id)]
    }

    /// The WebSocket finished its handshake.
    pub fn socket_opened(&mut self) {
        if self.state == SessionState::Connecting {
            self.state = SessionState::Open;
        }
    }

    /// The transport failed terminally (for example a 401 on the socket).
    pub fn socket_failed(&mut self, message: impl Into<String>) -> Vec<SessionEvent> {
        self.reconnect_at = None;
        self.state = SessionState::Failed;
        vec![SessionEvent::Failed(message.into())]
    }

    /// React to one relayed worker frame. Frames for another pane are ignored,
    /// because the gateway broadcasts every worker frame to every client.
    pub fn on_frame(&mut self, frame: &ServerFrame) -> Vec<SessionEvent> {
        if let Some(pane) = frame_pane(frame) {
            if Some(pane) != self.pane_id.as_deref() {
                return Vec::new();
            }
        }

        match frame {
            ServerFrame::PtyOpened { .. } => {
                self.state = SessionState::Live;
                self.live_once = true;
                self.backoff_attempt = 0;
                self.reconnect_at = None;
                vec![SessionEvent::Opened]
            }
            ServerFrame::PtyOutput { .. } => {
                if self.state == SessionState::Open {
                    self.state = SessionState::Live;
                }
                output_bytes(frame)
                    .map(SessionEvent::Output)
                    .into_iter()
                    .collect()
            }
            ServerFrame::PtyExit { code, .. } => {
                self.state = SessionState::Closed;
                self.reconnect_at = None;
                vec![SessionEvent::Exited(*code)]
            }
            ServerFrame::PtyError { message, .. } => {
                self.state = SessionState::Failed;
                self.reconnect_at = None;
                vec![SessionEvent::Failed(message.clone())]
            }
            ServerFrame::Pong => vec![SessionEvent::Pong],
            ServerFrame::Unknown => Vec::new(),
        }
    }

    /// The WebSocket closed or errored. Schedules a retry unless the tab was
    /// closed by the user, the shell exited, or the worker rejected the pane.
    pub fn socket_lost(&mut self, now_ms: u64) -> Vec<Plan> {
        if self.closed_by_user
            || self.reconnect_at.is_some()
            || matches!(self.state, SessionState::Failed)
        {
            return Vec::new();
        }
        if matches!(self.state, SessionState::Closed) && self.live_once {
            // `pty_exit` already moved us to Closed: the shell is gone, do not
            // resurrect it behind the user's back.
            return Vec::new();
        }

        self.state = SessionState::Closed;
        let ms = backoff_ms(self.backoff_attempt);
        self.backoff_attempt = self.backoff_attempt.saturating_add(1);
        self.reconnect_at = Some(now_ms.saturating_add(ms));
        vec![Plan::ReconnectIn { ms }]
    }

    /// Advance the clock. Starts a retry when its deadline has passed.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Plan> {
        let Some(deadline) = self.reconnect_at else {
            return Vec::new();
        };
        if now_ms < deadline {
            return Vec::new();
        }
        self.begin_retry()
    }

    /// Retry immediately, resetting the backoff. Used by the overlay's retry
    /// button and the reconnect shortcut.
    pub fn retry_now(&mut self) -> Vec<Plan> {
        self.closed_by_user = false;
        self.backoff_attempt = 0;
        self.reconnect_at = None;
        self.begin_retry()
    }

    fn begin_retry(&mut self) -> Vec<Plan> {
        self.reconnect_at = None;
        self.state = SessionState::Connecting;
        let pane_id = self.fresh_pane();
        let mut plans = Vec::with_capacity(2);
        if self.live_once {
            self.reconnect_count = self.reconnect_count.saturating_add(1);
            plans.push(Plan::Reconnected {
                pane_id: pane_id.clone(),
            });
        }
        plans.push(self.open_plan(pane_id));
        plans
    }

    fn fresh_pane(&mut self) -> String {
        let pane_id = format!("{}-{}", self.tag, self.generation);
        self.generation = self.generation.saturating_add(1);
        self.pane_id = Some(pane_id.clone());
        pane_id
    }

    fn open_plan(&self, pane_id: String) -> Plan {
        Plan::OpenPty {
            pane_id,
            cwd: self.cwd.clone(),
            cols: self.cols,
            rows: self.rows,
        }
    }

    /// Queue bytes for the pty. Only meaningful once the pane is live.
    pub fn input(&mut self, bytes: &[u8]) -> Option<Plan> {
        if bytes.is_empty() || !matches!(self.state, SessionState::Live) {
            return None;
        }
        let pane_id = self.pane_id.clone()?;
        Some(Plan::Input {
            pane_id,
            bytes: bytes.to_vec(),
        })
    }

    /// Report a new viewport size. The plan is only emitted when the size
    /// actually changed and a pane exists.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Plan> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if (cols, rows) == (self.cols, self.rows) {
            return None;
        }
        self.cols = cols;
        self.rows = rows;
        if !matches!(self.state, SessionState::Open | SessionState::Live) {
            return None;
        }
        let pane_id = self.pane_id.clone()?;
        Some(Plan::Resize {
            pane_id,
            cols,
            rows,
        })
    }

    /// Keepalive, used to measure round-trip latency.
    #[must_use]
    pub fn ping(&self) -> Option<Plan> {
        matches!(self.state, SessionState::Live).then_some(Plan::Ping)
    }

    /// User closed the tab: tell the worker and stop retrying.
    pub fn close(&mut self) -> Option<Plan> {
        self.closed_by_user = true;
        self.reconnect_at = None;
        self.state = SessionState::Closed;
        self.pane_id.clone().map(|pane_id| Plan::Close { pane_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ServerFrame;

    fn session() -> Session {
        Session::new("w1", "t1", "/workspace", 80, 24)
    }

    fn open_pty(plans: &[Plan]) -> (&str, u16, u16) {
        match plans.first().expect("open plan") {
            Plan::OpenPty {
                pane_id,
                cols,
                rows,
                ..
            } => (pane_id, *cols, *rows),
            other => panic!("expected OpenPty, got {other:?}"),
        }
    }

    fn frame(kind: &str, pane: &str) -> ServerFrame {
        serde_json::from_str(&format!(r#"{{"kind":"{kind}","pane_id":"{pane}"}}"#)).unwrap()
    }

    fn live() -> Session {
        let mut session = session();
        session.start();
        session.socket_opened();
        session.on_frame(&ServerFrame::PtyOpened {
            pane_id: "t1-0".into(),
        });
        session
    }

    #[test]
    fn start_emits_open_pty_with_config() {
        let mut session = session();
        let plans = session.start();
        let (pane, cols, rows) = open_pty(&plans);
        assert_eq!(pane, "t1-0");
        assert_eq!((cols, rows), (80, 24));
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.pane_id(), Some("t1-0"));
    }

    #[test]
    fn lifecycle_moves_connecting_open_live() {
        let mut session = session();
        session.start();
        assert_eq!(session.state(), SessionState::Connecting);
        session.socket_opened();
        assert_eq!(session.state(), SessionState::Open);
        let events = session.on_frame(&ServerFrame::PtyOpened {
            pane_id: "t1-0".into(),
        });
        assert_eq!(events, vec![SessionEvent::Opened]);
        assert_eq!(session.state(), SessionState::Live);
        assert!(session.live_once());
    }

    #[test]
    fn output_is_decoded_only_after_live() {
        let mut session = live();
        let events = session.on_frame(&ServerFrame::PtyOutput {
            pane_id: "t1-0".into(),
            data: "aGk=".into(),
        });
        assert_eq!(events, vec![SessionEvent::Output(b"hi".to_vec())]);
    }

    #[test]
    fn frames_for_other_panes_are_ignored() {
        let mut session = live();
        assert!(session
            .on_frame(&ServerFrame::PtyOutput {
                pane_id: "other-9".into(),
                data: "aGk=".into(),
            })
            .is_empty());
        assert!(session
            .on_frame(&ServerFrame::PtyExit {
                pane_id: "other-9".into(),
                code: Some(0),
            })
            .is_empty());
        assert_eq!(session.state(), SessionState::Live);
    }

    #[test]
    fn input_requires_a_live_pane() {
        let mut session = session();
        session.start();
        assert!(session.input(b"x").is_none());
        session.socket_opened();
        assert!(session.input(b"x").is_none());
        session.on_frame(&ServerFrame::PtyOpened {
            pane_id: "t1-0".into(),
        });
        let plan = session.input(b"x").unwrap();
        assert_eq!(
            plan,
            Plan::Input {
                pane_id: "t1-0".into(),
                bytes: b"x".to_vec()
            }
        );
        assert!(session.input(b"").is_none());
    }

    #[test]
    fn resize_is_emitted_only_on_change_and_when_connected() {
        let mut session = session();
        assert!(session.resize(100, 40).is_none());
        assert_eq!(session.size(), (100, 40));
        session.start();
        session.socket_opened();
        let plan = session.resize(120, 50).unwrap();
        assert_eq!(
            plan,
            Plan::Resize {
                pane_id: "t1-0".into(),
                cols: 120,
                rows: 50
            }
        );
        assert!(session.resize(120, 50).is_none());
    }

    #[test]
    fn socket_loss_schedules_backoff() {
        let mut session = live();
        assert_eq!(
            session.socket_lost(1_000),
            vec![Plan::ReconnectIn { ms: 500 }]
        );
        assert_eq!(session.state(), SessionState::Closed);
        assert_eq!(session.reconnect_at(), Some(1_500));
        assert!(session.tick(1_499).is_empty());
    }

    #[test]
    fn retry_opens_a_fresh_pane_and_marks_the_reconnect() {
        let mut session = live();
        session.socket_lost(1_000);
        let plans = session.tick(1_500);
        assert_eq!(
            plans,
            vec![
                Plan::Reconnected {
                    pane_id: "t1-1".into()
                },
                Plan::OpenPty {
                    pane_id: "t1-1".into(),
                    cwd: "/workspace".into(),
                    cols: 80,
                    rows: 24
                }
            ]
        );
        assert_eq!(session.pane_id(), Some("t1-1"));
        assert_eq!(session.state(), SessionState::Connecting);
        assert_eq!(session.reconnect_count(), 1);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(backoff_ms(0), 500);
        assert_eq!(backoff_ms(1), 1_000);
        assert_eq!(backoff_ms(2), 2_000);
        assert_eq!(backoff_ms(3), 4_000);
        assert_eq!(backoff_ms(4), 8_000);
        assert_eq!(backoff_ms(99), 8_000);
    }

    #[test]
    fn consecutive_failures_use_longer_delays() {
        let mut session = live();
        assert_eq!(session.socket_lost(0), vec![Plan::ReconnectIn { ms: 500 }]);
        session.tick(500);
        session.socket_opened();
        assert_eq!(
            session.socket_lost(1_000),
            vec![Plan::ReconnectIn { ms: 1_000 }]
        );
        session.tick(2_000);
        assert_eq!(
            session.socket_lost(3_000),
            vec![Plan::ReconnectIn { ms: 2_000 }]
        );
    }

    #[test]
    fn successful_open_resets_the_backoff() {
        let mut session = live();
        session.socket_lost(0);
        session.tick(500);
        session.socket_opened();
        let pane = session.pane_id().unwrap().to_string();
        session.on_frame(&ServerFrame::PtyOpened { pane_id: pane });
        session.socket_lost(2_000);
        assert_eq!(
            session.reconnect_at(),
            Some(2_500),
            "a live session should restart at the base delay"
        );
    }

    #[test]
    fn socket_loss_is_ignored_when_already_scheduled() {
        let mut session = live();
        assert_eq!(session.socket_lost(0).len(), 1);
        assert!(session.socket_lost(10).is_empty());
    }

    #[test]
    fn pty_exit_closes_without_reconnecting() {
        let mut session = live();
        let events = session.on_frame(&ServerFrame::PtyExit {
            pane_id: "t1-0".into(),
            code: Some(0),
        });
        assert_eq!(events, vec![SessionEvent::Exited(Some(0))]);
        assert_eq!(session.state(), SessionState::Closed);
        assert!(session.socket_lost(1_000).is_empty());
    }

    #[test]
    fn pty_error_fails_and_waits_for_a_manual_retry() {
        let mut session = live();
        let events = session.on_frame(&ServerFrame::PtyError {
            pane_id: "t1-0".into(),
            message: "spawn failed".into(),
        });
        assert_eq!(events, vec![SessionEvent::Failed("spawn failed".into())]);
        assert_eq!(session.state(), SessionState::Failed);
        assert!(session.socket_lost(1_000).is_empty());
    }

    #[test]
    fn retry_now_restarts_immediately_with_a_fresh_pane() {
        let mut session = session();
        session.start();
        session.socket_failed("denied");
        assert_eq!(session.state(), SessionState::Failed);
        let plans = session.retry_now();
        assert_eq!(
            plans,
            vec![Plan::OpenPty {
                pane_id: "t1-1".into(),
                cwd: "/workspace".into(),
                cols: 80,
                rows: 24
            }]
        );
        assert_eq!(session.state(), SessionState::Connecting);
    }

    #[test]
    fn close_sends_close_and_stops_retrying() {
        let mut session = live();
        assert_eq!(
            session.close(),
            Some(Plan::Close {
                pane_id: "t1-0".into()
            })
        );
        assert_eq!(session.state(), SessionState::Closed);
        assert!(session.socket_lost(1_000).is_empty());
        assert!(session.tick(9_999).is_empty());
    }

    #[test]
    fn close_before_start_has_no_plan() {
        let mut session = session();
        assert_eq!(session.close(), None);
    }

    #[test]
    fn ping_only_while_live() {
        let mut session = session();
        assert_eq!(session.ping(), None);
        session = live();
        assert_eq!(session.ping(), Some(Plan::Ping));
    }

    #[test]
    fn unknown_frames_are_ignored() {
        let mut session = live();
        assert!(session.on_frame(&ServerFrame::Unknown).is_empty());
        assert_eq!(session.state(), SessionState::Live);
    }

    #[test]
    fn pong_is_reported() {
        let mut session = live();
        assert_eq!(
            session.on_frame(&ServerFrame::Pong),
            vec![SessionEvent::Pong]
        );
    }

    #[test]
    fn frame_helper_builds_parseable_frames() {
        assert_eq!(
            frame("pty_opened", "t1-0"),
            ServerFrame::PtyOpened {
                pane_id: "t1-0".into()
            }
        );
    }
}
