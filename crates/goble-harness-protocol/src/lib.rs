//! Wire types for the harness boundary.
//!
//! This is the `protocol` layer of the `types <- protocol <- runtime` split. It
//! owns the serializable messages that cross the harness seam — the place where
//! a "bring your own harness" (an external CLI, or a remote agent) plugs in. The
//! natural transport is newline-delimited JSON over stdio (or over a socket);
//! this crate must stay free of any transport/execution concepts.

use goble_harness_types::{HarnessSnapshot, HarnessTurn, RemoteScreenConfig, SessionId};
use serde::{Deserialize, Serialize};

/// A tool description exposed by a harness (wire form, JSON-schema shaped).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchemaWire {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Requests the host sends to a harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HarnessClientRequest {
    ListTools,
    Run { turn: HarnessTurn },
    Resume {
        session_id: SessionId,
        response: String,
    },
    Cancel {
        session_id: SessionId,
    },
    Checkpoint {
        session_id: SessionId,
    },
    Restore {
        session_id: SessionId,
        snapshot: HarnessSnapshot,
    },
}

/// Events a harness streams back to the host. Mirrors the existing
/// `HarnessEvent` shape in `goble-core`, but defined here so the protocol layer
/// does not depend on `goble-core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", content = "payload")]
pub enum HarnessServerEvent {
    Ready,
    ToolList {
        tools: Vec<ToolSchemaWire>,
    },
    AssistantDelta {
        session_id: SessionId,
        delta: String,
    },
    ToolCallStarted {
        session_id: SessionId,
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolCallFinished {
        session_id: SessionId,
        id: String,
        result: String,
    },
    ToolCallError {
        session_id: SessionId,
        id: String,
        message: String,
    },
    AskUser {
        session_id: SessionId,
        question: String,
        quick_replies: Vec<String>,
    },
    /// A command tool is suspended waiting on the user's approval before it
    /// runs: `id` is the suspended call, `candidates` the proposed command
    /// lines and `cwd` where they would run. The host answers it with a
    /// command decision through the harness runtime's resume path.
    CommandProposed {
        session_id: SessionId,
        id: String,
        candidates: Vec<String>,
        cwd: String,
    },
    /// A sub-agent child was spawned on this session's conversation: the
    /// child's identity as its record carries it (`chat_id` is the parent
    /// conversation, the rest the spawn spec), mirrored from the core
    /// `HarnessEvent::SubAgentSpawned` as plain data.
    SubAgentSpawned {
        session_id: SessionId,
        chat_id: String,
        subagent_id: String,
        subagent_type: String,
        description: String,
        parent_call_id: String,
        run_in_background: bool,
    },
    /// A live sub-agent's record moved forward: status kind, activity line,
    /// budget counters and the elapsed time from the record's own clock.
    SubAgentProgress {
        session_id: SessionId,
        chat_id: String,
        subagent_id: String,
        status: String,
        activity: String,
        turns: u32,
        tool_calls: u32,
        tokens: u64,
        duration_ms: u64,
    },
    /// A sub-agent reached a terminal status: `output` for `completed`,
    /// `error` for `failed`/`cancelled`, with the record's final counters.
    SubAgentFinished {
        session_id: SessionId,
        chat_id: String,
        subagent_id: String,
        status: String,
        output: Option<String>,
        error: Option<String>,
        duration_ms: u64,
        turns: u32,
        tool_calls: u32,
        tokens: u64,
    },
    MissionUpdated {
        session_id: SessionId,
        mission_id: String,
        status: String,
    },
    /// One model call's token accounting, from the provider's own response.
    /// `cached` is the part of `input` the provider served from its prompt
    /// cache, when it reports one.
    TokenUsage {
        session_id: SessionId,
        chat_id: String,
        input: u64,
        cached: Option<u64>,
        output: u64,
    },
    /// The model began a reasoning (thinking) step, identified by `step`, in the
    /// current thinking `mode`. Carried so a host can render thinking live.
    ReasoningStarted {
        session_id: SessionId,
        step: usize,
        mode: String,
    },
    /// A chunk of the current reasoning step's text. The step is identified by
    /// the preceding `ReasoningStarted`; deltas stream in order until
    /// `ReasoningDone`.
    ReasoningDelta {
        session_id: SessionId,
        delta: String,
    },
    /// The reasoning step finished: `content` is the step's full text and
    /// `decision` the tool-call decision it settled on.
    ReasoningDone {
        session_id: SessionId,
        step: usize,
        mode: String,
        content: String,
        decision: String,
    },
    Done {
        session_id: SessionId,
    },
    Error {
        session_id: SessionId,
        message: String,
    },
    /// The harness's reply to a [`HarnessClientRequest::Checkpoint`]: a snapshot
    /// of its execution state the host can hand back on `Restore` to rewind.
    Checkpoint {
        session_id: SessionId,
        snapshot: HarnessSnapshot,
    },
    /// The harness asks the host to open an interactive remote desktop over the
    /// terminal/harness window. Emitted when the agent decides to hand off to a
    /// GUI session; the host opens the stream (via `goble-screen-sdk`) and shows
    /// it in a screen pane, routing input back through it.
    ScreenHandoff {
        session_id: SessionId,
        config: RemoteScreenConfig,
    },
}

/// The tool a harness exposes so the agent can request the interactive handoff.
pub const OPEN_SCREEN_TOOL: &str = "open_screen";

impl HarnessServerEvent {
    /// Build a [`HarnessServerEvent::ScreenHandoff`] from an `open_screen` tool
    /// call. The tool arguments carry `host` (required), `port` (default 3389),
    /// `username`, `password`, and optional `width`/`height` (default 1280x720).
    ///
    /// Returns `None` if the arguments are not parseable as a screen request so
    /// the caller can surface a `ToolCallError` instead.
    pub fn screen_handoff_from_tool_call(
        session_id: SessionId,
        arguments: &serde_json::Value,
    ) -> Option<Self> {
        let host = arguments.get("host")?.as_str()?;
        let username = arguments
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let password = arguments
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let port = arguments
            .get("port")
            .and_then(|v| v.as_u64())
            .map(|p| p as u16)
            .unwrap_or(3389);
        let width = arguments
            .get("width")
            .and_then(|v| v.as_u64())
            .map(|w| w as u16)
            .unwrap_or(1280);
        let height = arguments
            .get("height")
            .and_then(|v| v.as_u64())
            .map(|h| h as u16)
            .unwrap_or(720);
        Some(Self::ScreenHandoff {
            session_id,
            config: RemoteScreenConfig {
                host: host.to_string(),
                port,
                username: username.to_string(),
                password: password.to_string(),
                width,
                height,
            },
        })
    }
}

/// One frame on the harness wire: either a host request or a harness event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "direction", rename_all = "snake_case")]
pub enum HarnessMessage {
    Client(HarnessClientRequest),
    Server(HarnessServerEvent),
}

impl HarnessMessage {
    /// Encode one frame as a single line of JSON.
    pub fn to_line(&self) -> anyhow::Result<String> {
        let mut line = serde_json::to_string(self)?;
        line.push('\n');
        Ok(line)
    }

    /// Decode a single line of JSON into a frame.
    pub fn from_line(line: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str::<Self>(line.trim())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_types::{HarnessId, HarnessTurn};

    #[test]
    fn client_request_run_roundtrip() {
        let turn = HarnessTurn::new(HarnessId::new("cli"), SessionId::new("s1"), "fix bug");
        let msg = HarnessMessage::Client(HarnessClientRequest::Run { turn });
        let line = msg.to_line().unwrap();
        let decoded = HarnessMessage::from_line(&line).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn server_event_roundtrip() {
        let msg = HarnessMessage::Server(HarnessServerEvent::AssistantDelta {
            session_id: SessionId::new("s1"),
            delta: "hello".into(),
        });
        let line = msg.to_line().unwrap();
        let decoded = HarnessMessage::from_line(&line).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn command_proposed_event_roundtrips() {
        // The approval suspension crosses the harness wire as its own frame
        // instead of being dropped at the adaptor.
        let ev = HarnessMessage::Server(HarnessServerEvent::CommandProposed {
            session_id: SessionId::new("s1"),
            id: "call-1".into(),
            candidates: vec!["git status".into(), "git diff --stat".into()],
            cwd: "/workspace".into(),
        });
        let line = ev.to_line().unwrap();
        assert_eq!(HarnessMessage::from_line(&line).unwrap(), ev);
    }

    #[test]
    fn reasoning_events_roundtrip() {
        let events = vec![
            HarnessServerEvent::ReasoningStarted {
                session_id: SessionId::new("s1"),
                step: 0,
                mode: "contemplating".into(),
            },
            HarnessServerEvent::ReasoningDelta {
                session_id: SessionId::new("s1"),
                delta: "weighing options".into(),
            },
            HarnessServerEvent::ReasoningDone {
                session_id: SessionId::new("s1"),
                step: 0,
                mode: "contemplating".into(),
                content: "weighing options".into(),
                decision: "\"execute\"".into(),
            },
        ];
        for ev in events {
            let msg = HarnessMessage::Server(ev.clone());
            let line = msg.to_line().unwrap();
            let decoded = HarnessMessage::from_line(&line).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn wire_does_not_depend_on_core_types() {
        // The protocol layer only re-serializes the harness-turn shape; a
        // ToolList can be carried without importing ToolSchemaWire internals.
        let tools = vec![ToolSchemaWire {
            name: "read_file".into(),
            description: "read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let msg = HarnessMessage::Server(HarnessServerEvent::ToolList { tools });
        let line = msg.to_line().unwrap();
        assert!(line.contains("read_file"));
    }

    #[test]
    fn reversible_checkpoint_roundtrip() {
        let snapshot = HarnessSnapshot::new("counter", serde_json::json!({ "count": 3 }));

        // The host asks the harness to checkpoint.
        let req = HarnessMessage::Client(HarnessClientRequest::Checkpoint {
            session_id: SessionId::new("s1"),
        });
        let line = req.to_line().unwrap();
        assert_eq!(HarnessMessage::from_line(&line).unwrap(), req);

        // The harness returns the snapshot.
        let ev = HarnessMessage::Server(HarnessServerEvent::Checkpoint {
            session_id: SessionId::new("s1"),
            snapshot: snapshot.clone(),
        });
        let line = ev.to_line().unwrap();
        assert_eq!(HarnessMessage::from_line(&line).unwrap(), ev);

        // The host asks to restore from the snapshot.
        let restore = HarnessMessage::Client(HarnessClientRequest::Restore {
            session_id: SessionId::new("s1"),
            snapshot,
        });
        let line = restore.to_line().unwrap();
        assert_eq!(HarnessMessage::from_line(&line).unwrap(), restore);
    }

    #[test]
    fn screen_handoff_event_roundtrips() {
        let ev = HarnessMessage::Server(HarnessServerEvent::ScreenHandoff {
            session_id: SessionId::new("s1"),
            config: RemoteScreenConfig::new("vm.example.com", "user", "pass"),
        });
        let line = ev.to_line().unwrap();
        let decoded = HarnessMessage::from_line(&line).unwrap();
        assert_eq!(decoded, ev);
    }

    #[test]
    fn screen_handoff_from_tool_call_parses_defaults() {
        // Minimal args: only host. Ports/dims fall back to defaults.
        let ev = HarnessServerEvent::screen_handoff_from_tool_call(
            SessionId::new("s1"),
            &serde_json::json!({ "host": "vm.example.com", "username": "u", "password": "p" }),
        )
        .unwrap();
        match ev {
            HarnessServerEvent::ScreenHandoff { config, .. } => {
                assert_eq!(config.host, "vm.example.com");
                assert_eq!(config.port, 3389);
                assert_eq!(config.width, 1280);
                assert_eq!(config.height, 720);
                assert_eq!(config.username, "u");
                assert_eq!(config.password, "p");
            }
            _ => panic!("expected a screen handoff"),
        }
    }

    #[test]
    fn screen_handoff_from_tool_call_requires_host() {
        let ev = HarnessServerEvent::screen_handoff_from_tool_call(
            SessionId::new("s1"),
            &serde_json::json!({ "port": 3390 }),
        );
        assert!(ev.is_none());
    }

    #[test]
    fn capabilities_reversible_flag_roundtrips() {
        let mut caps = goble_harness_types::HarnessCapabilities::internal();
        caps.reversible = true;
        let json = serde_json::to_string(&caps).unwrap();
        let decoded: goble_harness_types::HarnessCapabilities =
            serde_json::from_str(&json).unwrap();
        assert!(decoded.reversible);

        // A payload without the field deserializes to the default (false).
        let legacy = r#"{"voice":false,"screen":false,"sandbox_level":"allow_list","tools":[]}"#;
        let decoded: goble_harness_types::HarnessCapabilities =
            serde_json::from_str(legacy).unwrap();
        assert!(!decoded.reversible);
    }
}
