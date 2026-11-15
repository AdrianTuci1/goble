//! The cloud agent viewer: its transcript and one turn at a time.
//!
//! Pure state — the DOM layer renders [`Viewer::rows`] and feeds it relayed
//! frames. A turn is keyed by its `trace_id`, so frames from another turn,
//! another pane or another client are ignored. Nothing here touches
//! `web_sys`, so the streaming and settling rules are tested on the host.

use crate::protocol::{ServerFrame, TurnStatus};

/// A worker that never answers a turn with `agent_started` leaves the viewer
/// waiting; the phone gives up after the same window and says so.
pub const START_TIMEOUT_MS: f64 = 8_000.0;

/// A harness that only reports its answer at the end sends `agent_finished`
/// before `thread_agent_reply`; the viewer waits this long for that text
/// before it closes the turn.
pub const REPLY_GRACE_MS: f64 = 400.0;

/// How a transcript row draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// What the user typed: a bubble on the right.
    User,
    /// Streamed answer text: plain paragraphs on the left.
    Assistant,
    /// One tool call, compact and muted.
    Tool,
    /// A failure: red.
    Error,
    /// Status the viewer reports itself: muted.
    Notice,
}

/// One line of the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub text: String,
}

/// The turn the viewer is waiting on.
#[derive(Debug, Clone)]
struct Turn {
    trace_id: String,
    started: bool,
    /// The row deltas append to, created with the first delta of a paragraph.
    /// A tool call closes the paragraph so later text lands below it.
    assistant: Option<usize>,
    /// Tool call ids and the rows they draw, so finish/error can update them.
    tools: Vec<(String, usize)>,
    sent_at: f64,
    /// Set once the run finished without streamed text; the reply may still be
    /// on its way until this deadline.
    reply_by: Option<f64>,
}

/// The transcript and turn state of one viewer pane.
#[derive(Debug, Default)]
pub struct Viewer {
    rows: Vec<Row>,
    turn: Option<Turn>,
}

impl Viewer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// A viewer with no rows draws the landing instead of a transcript.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// A turn is in flight: the composer offers Stop rather than Send.
    #[must_use]
    pub fn running(&self) -> bool {
        self.turn.is_some()
    }

    /// The trace the viewer is listening for, while a turn runs.
    #[must_use]
    pub fn trace_id(&self) -> Option<&str> {
        self.turn.as_ref().map(|turn| turn.trace_id.as_str())
    }

    /// Start a turn: the prompt becomes a user row and the viewer awaits the
    /// stream carrying `trace_id`. A turn already in flight is left alone.
    pub fn begin(&mut self, trace_id: &str, prompt: &str, now: f64) -> bool {
        if self.turn.is_some() {
            return false;
        }
        self.push(RowKind::User, prompt);
        self.turn = Some(Turn {
            trace_id: trace_id.to_string(),
            started: false,
            assistant: None,
            tools: Vec::new(),
            sent_at: now,
            reply_by: None,
        });
        true
    }

    /// Stop watching the turn in flight. The worker keeps running; late frames
    /// for this trace no longer match anything.
    pub fn stop(&mut self) -> bool {
        if self.turn.take().is_none() {
            return false;
        }
        self.push(
            RowKind::Notice,
            "Stopped watching this turn. The worker keeps running.",
        );
        true
    }

    /// Record a viewer-level failure, e.g. the socket dropping mid-turn.
    pub fn fail(&mut self, message: &str) -> bool {
        self.turn = None;
        self.push(RowKind::Error, message);
        true
    }

    /// Feed one relayed frame. Returns whether the transcript changed.
    pub fn on_frame(&mut self, frame: &ServerFrame, now: f64) -> bool {
        let Some(trace) = self.trace().map(str::to_string) else {
            return false;
        };
        match frame {
            ServerFrame::AgentStarted { trace_id, .. } => {
                if trace_id != &trace {
                    return false;
                }
                if let Some(turn) = self.turn.as_mut() {
                    turn.started = true;
                }
                false
            }
            ServerFrame::AssistantDelta { trace_id, delta } => {
                if trace_id != &trace || delta.is_empty() {
                    return false;
                }
                self.mark_started();
                self.append_assistant(delta);
                true
            }
            ServerFrame::ThreadAgentReply {
                trace_id, content, ..
            } => {
                if trace_id != &trace {
                    return false;
                }
                self.mark_started();
                if !content.is_empty() {
                    self.replace_assistant(content);
                }
                self.turn = None;
                true
            }
            ServerFrame::AgentFinished { trace_id, status } => {
                if trace_id != &trace {
                    return false;
                }
                match status {
                    TurnStatus::Failure(reason) => {
                        self.turn = None;
                        self.push(RowKind::Error, format!("The turn failed: {reason}"));
                    }
                    TurnStatus::Cancelled => {
                        self.turn = None;
                        self.push(RowKind::Notice, "The turn was cancelled.");
                    }
                    TurnStatus::Success => {
                        let streamed = self.turn.as_ref().and_then(|turn| turn.assistant).is_some();
                        if streamed {
                            self.turn = None;
                            self.push(RowKind::Notice, "The turn finished.");
                        } else if let Some(turn) = self.turn.as_mut() {
                            turn.reply_by = Some(now + REPLY_GRACE_MS);
                        }
                    }
                    TurnStatus::Pending | TurnStatus::Running => {}
                }
                true
            }
            ServerFrame::AgentLog {
                trace_id,
                level,
                message,
                ..
            } => {
                if trace_id != &trace || message.is_empty() {
                    return false;
                }
                match level.as_str() {
                    "error" => {
                        self.push(RowKind::Error, message.clone());
                    }
                    "warn" => {
                        self.push(RowKind::Notice, message.clone());
                    }
                    _ => return false,
                }
                true
            }
            ServerFrame::ToolCallStarted {
                trace_id, id, name, ..
            } => {
                if trace_id != &trace {
                    return false;
                }
                self.mark_started();
                if let Some(turn) = self.turn.as_mut() {
                    // Text after the call is a new paragraph, below the row.
                    turn.assistant = None;
                }
                let row = self.push(RowKind::Tool, format!("{name} · running"));
                if let Some(turn) = self.turn.as_mut() {
                    turn.tools.push((id.clone(), row));
                }
                true
            }
            ServerFrame::ToolCallFinished { trace_id, id, .. } => {
                if trace_id != &trace {
                    return false;
                }
                self.set_tool(id, "done").is_some()
            }
            ServerFrame::ToolCallError {
                trace_id,
                id,
                message,
            } => {
                if trace_id != &trace {
                    return false;
                }
                let state = format!("failed — {}", first_line(message));
                self.set_tool(id, &state).is_some()
            }
            ServerFrame::AskUser {
                trace_id,
                question,
                quick_replies,
            } => {
                if trace_id != &trace {
                    return false;
                }
                self.mark_started();
                let mut text = format!("Question: {question}");
                if !quick_replies.is_empty() {
                    text.push_str(&format!(" · replies: {}", quick_replies.join(" / ")));
                }
                self.push(RowKind::Notice, text);
                true
            }
            ServerFrame::PtyOpened { .. }
            | ServerFrame::PtyOutput { .. }
            | ServerFrame::PtyExit { .. }
            | ServerFrame::PtyError { .. }
            | ServerFrame::Pong
            | ServerFrame::Unknown => false,
        }
    }

    /// Advance the clock: give up on a turn that never started, and settle a
    /// finished run that never sent streamed text.
    pub fn tick(&mut self, now: f64) -> bool {
        let Some(turn) = self.turn.as_ref() else {
            return false;
        };
        if let Some(deadline) = turn.reply_by {
            if now >= deadline {
                self.turn = None;
                self.push(RowKind::Notice, "The turn finished.");
                return true;
            }
            return false;
        }
        if turn.started || now - turn.sent_at < START_TIMEOUT_MS {
            return false;
        }
        self.turn = None;
        self.push(RowKind::Error, "The worker did not start the turn.");
        true
    }

    fn trace(&self) -> Option<&str> {
        self.trace_id()
    }

    fn mark_started(&mut self) {
        if let Some(turn) = self.turn.as_mut() {
            turn.started = true;
        }
    }

    fn push(&mut self, kind: RowKind, text: impl Into<String>) -> usize {
        self.rows.push(Row {
            kind,
            text: text.into(),
        });
        self.rows.len() - 1
    }

    fn append_assistant(&mut self, delta: &str) {
        match self.turn.as_ref().and_then(|turn| turn.assistant) {
            Some(index) => {
                if let Some(row) = self.rows.get_mut(index) {
                    row.text.push_str(delta);
                }
            }
            None => {
                let index = self.push(RowKind::Assistant, delta);
                if let Some(turn) = self.turn.as_mut() {
                    turn.assistant = Some(index);
                }
            }
        }
    }

    fn replace_assistant(&mut self, text: &str) {
        match self.turn.as_ref().and_then(|turn| turn.assistant) {
            Some(index) => {
                if let Some(row) = self.rows.get_mut(index) {
                    row.text = text.to_string();
                }
            }
            None => {
                let index = self.push(RowKind::Assistant, text);
                if let Some(turn) = self.turn.as_mut() {
                    turn.assistant = Some(index);
                }
            }
        }
    }

    /// Rewrite a tool row's state, keeping the name it started with.
    fn set_tool(&mut self, id: &str, state: &str) -> Option<()> {
        let index = self
            .turn
            .as_ref()?
            .tools
            .iter()
            .find(|(call, _)| call == id)
            .map(|(_, row)| *row)?;
        let row = self.rows.get_mut(index)?;
        let name = row.text.split(" · ").next().unwrap_or("tool").to_string();
        row.text = format!("{name} · {state}");
        Some(())
    }
}

/// The first line of a message, so a tool failure stays one compact row.
fn first_line(message: &str) -> String {
    let line = message.lines().next().unwrap_or("").trim();
    let mut text: String = line.chars().take(120).collect();
    if line.chars().count() > 120 {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ServerFrame;

    fn started(trace: &str) -> ServerFrame {
        ServerFrame::AgentStarted {
            trace_id: trace.into(),
            agent_id: "a1".into(),
        }
    }

    fn delta(trace: &str, text: &str) -> ServerFrame {
        ServerFrame::AssistantDelta {
            trace_id: trace.into(),
            delta: text.into(),
        }
    }

    fn finished(trace: &str, status: TurnStatus) -> ServerFrame {
        ServerFrame::AgentFinished {
            trace_id: trace.into(),
            status,
        }
    }

    fn reply(trace: &str, content: &str) -> ServerFrame {
        ServerFrame::ThreadAgentReply {
            trace_id: trace.into(),
            thread_id: "th1".into(),
            content: content.into(),
        }
    }

    fn texts(viewer: &Viewer) -> Vec<(RowKind, String)> {
        viewer
            .rows()
            .iter()
            .map(|row| (row.kind, row.text.clone()))
            .collect()
    }

    #[test]
    fn a_new_viewer_is_empty_and_idle() {
        let viewer = Viewer::new();
        assert!(viewer.is_empty());
        assert!(!viewer.running());
        assert!(viewer.rows().is_empty());
    }

    #[test]
    fn begin_adds_the_user_row_and_starts_the_turn() {
        let mut viewer = Viewer::new();
        assert!(viewer.begin("t1", "hello", 0.0));
        assert!(viewer.running());
        assert!(!viewer.is_empty());
        assert_eq!(texts(&viewer), vec![(RowKind::User, "hello".to_string())]);
        assert_eq!(viewer.trace_id(), Some("t1"));
    }

    #[test]
    fn deltas_stream_into_one_lazily_created_assistant_row() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        assert!(!viewer.on_frame(&started("t1"), 1.0), "no row is drawn");
        assert!(viewer.on_frame(&delta("t1", "Hel"), 1.0));
        assert!(viewer.on_frame(&delta("t1", "lo"), 1.0));
        assert_eq!(
            texts(&viewer),
            vec![
                (RowKind::User, "hi".to_string()),
                (RowKind::Assistant, "Hello".to_string()),
            ]
        );
    }

    #[test]
    fn frames_for_another_turn_are_ignored() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        assert!(!viewer.on_frame(&delta("other", "nope"), 1.0));
        assert!(!viewer.on_frame(&finished("other", TurnStatus::Success), 1.0));
        assert!(viewer.running());
        assert_eq!(viewer.rows().len(), 1);
    }

    #[test]
    fn frames_before_a_turn_are_ignored() {
        let mut viewer = Viewer::new();
        assert!(!viewer.on_frame(&delta("t1", "nope"), 0.0));
        assert!(viewer.rows().is_empty());
    }

    #[test]
    fn a_reply_only_harness_fills_the_answer_and_settles() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&started("t1"), 1.0);
        viewer.on_frame(&finished("t1", TurnStatus::Success), 2.0);
        assert!(viewer.running(), "the reply may still be in flight");
        assert!(viewer.on_frame(&reply("t1", "the answer"), 2.1));
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer),
            vec![
                (RowKind::User, "hi".to_string()),
                (RowKind::Assistant, "the answer".to_string()),
            ]
        );
    }

    #[test]
    fn the_reply_replaces_streamed_text() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&delta("t1", "part"), 1.0);
        viewer.on_frame(&reply("t1", "complete"), 2.0);
        assert_eq!(
            texts(&viewer),
            vec![
                (RowKind::User, "hi".to_string()),
                (RowKind::Assistant, "complete".to_string()),
            ]
        );
    }

    #[test]
    fn a_streamed_turn_settles_on_finish() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&delta("t1", "text"), 1.0);
        assert!(viewer.on_frame(&finished("t1", TurnStatus::Success), 2.0));
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Notice, "The turn finished.".to_string()))
        );
    }

    #[test]
    fn a_failure_ends_the_turn_with_a_red_row() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&started("t1"), 1.0);
        viewer.on_frame(
            &finished("t1", TurnStatus::Failure("no model key".into())),
            2.0,
        );
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Error, "The turn failed: no model key".to_string()))
        );
    }

    #[test]
    fn a_finished_delta_less_turn_settles_after_the_grace() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&finished("t1", TurnStatus::Success), 2.0);
        assert!(viewer.running());
        assert!(!viewer.tick(2.0 + REPLY_GRACE_MS - 1.0));
        assert!(viewer.tick(2.0 + REPLY_GRACE_MS));
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Notice, "The turn finished.".to_string()))
        );
    }

    #[test]
    fn a_cancelled_turn_is_muted() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&finished("t1", TurnStatus::Cancelled), 1.0);
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Notice, "The turn was cancelled.".to_string()))
        );
    }

    #[test]
    fn a_turn_that_never_starts_fails_after_the_timeout() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        assert!(!viewer.tick(START_TIMEOUT_MS - 1.0));
        assert!(viewer.tick(START_TIMEOUT_MS));
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer).last(),
            Some(&(
                RowKind::Error,
                "The worker did not start the turn.".to_string()
            ))
        );
    }

    #[test]
    fn a_started_turn_never_times_out() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&started("t1"), 10.0);
        assert!(!viewer.tick(START_TIMEOUT_MS * 4.0));
        assert!(viewer.running());
    }

    #[test]
    fn tool_calls_draw_compact_rows_and_update_in_place() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(
            &ServerFrame::ToolCallStarted {
                trace_id: "t1".into(),
                id: "c1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "a"}),
            },
            1.0,
        );
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Tool, "read_file · running".to_string()))
        );
        viewer.on_frame(
            &ServerFrame::ToolCallFinished {
                trace_id: "t1".into(),
                id: "c1".into(),
                result: "ok".into(),
            },
            2.0,
        );
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Tool, "read_file · done".to_string()))
        );
        viewer.on_frame(
            &ServerFrame::ToolCallError {
                trace_id: "t1".into(),
                id: "c1".into(),
                message: "no such file\nsecond line".into(),
            },
            3.0,
        );
        assert_eq!(
            texts(&viewer).last(),
            Some(&(
                RowKind::Tool,
                "read_file · failed — no such file".to_string()
            ))
        );
    }

    #[test]
    fn text_after_a_tool_call_opens_a_new_paragraph() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(&delta("t1", "before"), 1.0);
        viewer.on_frame(
            &ServerFrame::ToolCallStarted {
                trace_id: "t1".into(),
                id: "c1".into(),
                name: "grep".into(),
                arguments: serde_json::Value::Null,
            },
            2.0,
        );
        viewer.on_frame(&delta("t1", "after"), 3.0);
        assert_eq!(
            texts(&viewer),
            vec![
                (RowKind::User, "hi".to_string()),
                (RowKind::Assistant, "before".to_string()),
                (RowKind::Tool, "grep · running".to_string()),
                (RowKind::Assistant, "after".to_string()),
            ]
        );
    }

    #[test]
    fn an_error_log_is_red_and_a_warning_is_muted() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(
            &ServerFrame::AgentLog {
                trace_id: "t1".into(),
                step_id: "s1".into(),
                level: "error".into(),
                message: "boom".into(),
            },
            1.0,
        );
        viewer.on_frame(
            &ServerFrame::AgentLog {
                trace_id: "t1".into(),
                step_id: "s1".into(),
                level: "info".into(),
                message: "ignored".into(),
            },
            1.0,
        );
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Error, "boom".to_string()))
        );
        assert_eq!(viewer.rows().len(), 2);
    }

    #[test]
    fn an_asked_question_is_shown_read_only() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        viewer.on_frame(
            &ServerFrame::AskUser {
                trace_id: "t1".into(),
                question: "which branch?".into(),
                quick_replies: vec!["main".into(), "dev".into()],
            },
            1.0,
        );
        assert_eq!(
            texts(&viewer).last(),
            Some(&(
                RowKind::Notice,
                "Question: which branch? · replies: main / dev".to_string()
            ))
        );
    }

    #[test]
    fn stopping_keeps_the_rows_and_closes_the_turn() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        assert!(viewer.stop());
        assert!(!viewer.running());
        assert_eq!(viewer.rows().len(), 2);
        assert!(!viewer.stop());
        // A late frame for the stopped turn matches nothing.
        assert!(!viewer.on_frame(&delta("t1", "late"), 1.0));
    }

    #[test]
    fn a_second_turn_appends_below_the_first() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "one", 0.0);
        viewer.on_frame(&reply("t1", "first answer"), 1.0);
        viewer.begin("t2", "two", 2.0);
        viewer.on_frame(&reply("t2", "second answer"), 3.0);
        assert_eq!(
            texts(&viewer),
            vec![
                (RowKind::User, "one".to_string()),
                (RowKind::Assistant, "first answer".to_string()),
                (RowKind::User, "two".to_string()),
                (RowKind::Assistant, "second answer".to_string()),
            ]
        );
    }

    #[test]
    fn begin_is_ignored_while_a_turn_runs() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "one", 0.0);
        assert!(!viewer.begin("t2", "two", 1.0));
        assert_eq!(viewer.rows().len(), 1);
    }

    #[test]
    fn pty_and_unknown_frames_do_not_touch_the_transcript() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        for frame in [
            ServerFrame::PtyOutput {
                pane_id: "p".into(),
                data: "aGk=".into(),
            },
            ServerFrame::Pong,
            ServerFrame::Unknown,
        ] {
            assert!(!viewer.on_frame(&frame, 1.0));
        }
        assert_eq!(viewer.rows().len(), 1);
    }

    #[test]
    fn a_socket_failure_ends_the_turn_with_its_message() {
        let mut viewer = Viewer::new();
        viewer.begin("t1", "hi", 0.0);
        assert!(viewer.fail("Connection closed (1006)."));
        assert!(!viewer.running());
        assert_eq!(
            texts(&viewer).last(),
            Some(&(RowKind::Error, "Connection closed (1006).".to_string()))
        );
    }
}
