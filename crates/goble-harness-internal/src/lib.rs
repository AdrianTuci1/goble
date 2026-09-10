//! BYOH seam adaptor: wraps a configured [`goble_core::harness::Harness`]
//! as a [`goble_harness_runtime::HarnessRuntime`].
//!
//! This is the piece that lets a framework-agnostic host drive the internal
//! agent (goble-core) through the same object-safe harness contract it uses for
//! external/CLI harnesses. It owns the translation between the core
//! [`HarnessEvent`] stream and the wire-shaped [`HarnessServerEvent`] frames the
//! protocol layer speaks, carrying the `Reasoning*` variants through and
//! dropping only `ThinkingModeChanged` (which the wire shape does not carry).
//!
//! [`InternalHarness`] holds a fully-configured `goble_core::Harness` (store +
//! resolved [`LlmProvider`] + model + web-search/runner/reasoning) and surfaces
//! it as [`HarnessRuntime`]. The adaptor never depends on `app/`, so it can be
//! wired in by either daemon composition root.

use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures::StreamExt;
use goble_core::harness::{CommandRunner, Harness, HarnessEvent, WebSearchConfig};
use goble_core::llm::LlmProvider;
use goble_core::store::Store;
use goble_harness_protocol::{HarnessServerEvent, OPEN_SCREEN_TOOL};
use goble_harness_runtime::{HarnessRun, HarnessRuntime};
use goble_harness_types::{HarnessCapabilities, HarnessId, HarnessTurn, SessionId};

/// The seam adaptor: an internal [`Harness`] behind the [`HarnessRuntime`] trait.
pub struct InternalHarness {
    id: HarnessId,
    provider: String,
    model: String,
    harness: Harness,
}

impl InternalHarness {
    /// Build an adaptor around a fresh [`Harness`] backed by `store`.
    ///
    /// Defaults to a mock provider/model so a run streams without any external
    /// LLM configuration; override with [`Self::with_llm`] /
    /// [`Self::with_provider`] / [`Self::with_model`].
    pub fn new(store: Store) -> Self {
        Self {
            id: HarnessId::default(),
            provider: "mock".to_string(),
            model: "mock".to_string(),
            harness: Harness::new(store),
        }
    }

    /// Wrap a fully-configured [`Harness`] (store + resolved llm + model +
    /// web-search/runner/reasoning) constructed by a composition root.
    pub fn from_harness(
        id: HarnessId,
        provider: impl Into<String>,
        model: impl Into<String>,
        harness: Harness,
    ) -> Self {
        Self {
            id,
            provider: provider.into(),
            model: model.into(),
            harness,
        }
    }

    pub fn cancel(&self) {
        self.harness.cancel();
    }

    /// The provider label carried on each completion request. The actual
    /// completion goes through the resolved [`LlmProvider`] on the inner
    /// [`Harness`]; this string is only the request's `provider` metadata.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_id(mut self, id: HarnessId) -> Self {
        self.id = id;
        self
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.harness = self.harness.with_llm(llm);
        self
    }

    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.harness = self.harness.with_runner(runner);
        self
    }

    pub fn with_reasoning(mut self, enabled: bool) -> Self {
        self.harness = self.harness.with_reasoning(enabled);
        self
    }

    pub fn with_auto_approve(mut self, auto_approve: bool) -> Self {
        self.harness = self.harness.with_auto_approve(auto_approve);
        self
    }

    pub fn with_web_search(mut self, config: WebSearchConfig) -> Self {
        self.harness = self.harness.with_web_search(config);
        self
    }

    /// Set the agent's workspace directory, forwarded to the inner
    /// [`goble_core::harness::Harness`].
    pub fn with_workspace_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.harness = self.harness.with_workspace_dir(dir);
        self
    }

    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.harness = self.harness.with_cancel(cancel);
        self
    }

    /// Resume a turn that suspended on a user answer.
    ///
    /// The daemon's `Resume` request carries the response; provider/model are
    /// the adaptor's configured values. The mapped event stream is wrapped as a
    /// [`HarnessRun`].
    pub fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> Option<HarnessRun> {
        Some(HarnessRun {
            events: map_stream(
                self.harness.resume_turn(
                    &session_id.0,
                    response,
                    credential,
                    &self.provider,
                    &self.model,
                ),
                session_id,
            ),
        })
    }
}

impl HarnessRuntime for InternalHarness {
    fn id(&self) -> HarnessId {
        self.id.clone()
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::internal()
    }

    fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
        // Cancellation flows through the inner Harness's own flag (set via
        // `with_cancel` / [`InternalHarness::cancel`]); the `cancel` arg keeps
        // the object-safe contract's signature stable.
        let session_id = turn.session_id.clone();
        let events = map_stream(
            self.harness
                .run_turn(&session_id.0, &turn.goal, &self.provider, &self.model),
            &session_id,
        );
        HarnessRun { events }
    }

    fn cancel(&self) {
        self.cancel();
    }

    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> Option<HarnessRun> {
        self.resume(session_id, response, credential)
    }
}

/// Map a core [`HarnessEvent`] stream into the wire [`HarnessServerEvent`] shape,
/// carrying the `Reasoning*` variants through and dropping only
/// `ThinkingModeChanged`, which the wire does not carry.
fn map_stream(
    events: Pin<Box<dyn futures::Stream<Item = HarnessEvent> + Send>>,
    session_id: &SessionId,
) -> Pin<Box<dyn futures::Stream<Item = HarnessServerEvent> + Send>> {
    let session_id = session_id.clone();
    Box::pin(events.flat_map(move |event| {
        let session_id = session_id.clone();
        futures::stream::iter(map_event(&session_id, event))
    }))
}

/// Map a core [`HarnessEvent`] into zero or more wire [`HarnessServerEvent`]s.
///
/// An `open_screen` tool call maps to **two** frames: the usual `ToolCallStarted`
/// mirror (so the agent's intent stays visible) plus a `ScreenHandoff` request
/// the host turns into a live desktop stream. Every other event maps to a single
/// frame (or none, for `ThinkingModeChanged`).
fn map_event(session_id: &SessionId, event: HarnessEvent) -> Vec<HarnessServerEvent> {
    match event {
        HarnessEvent::AssistantDelta(delta) => vec![HarnessServerEvent::AssistantDelta {
            session_id: session_id.clone(),
            delta,
        }],
        HarnessEvent::ToolCallStarted {
            id,
            name,
            arguments,
        } => {
            let mut out = vec![HarnessServerEvent::ToolCallStarted {
                session_id: session_id.clone(),
                id,
                name: name.clone(),
                arguments: arguments.clone(),
            }];
            if name == OPEN_SCREEN_TOOL {
                if let Some(handoff) =
                    HarnessServerEvent::screen_handoff_from_tool_call(session_id.clone(), &arguments)
                {
                    out.push(handoff);
                }
            }
            out
        }
        HarnessEvent::ToolCallFinished { id, result } => vec![HarnessServerEvent::ToolCallFinished {
            session_id: session_id.clone(),
            id,
            result,
        }],
        HarnessEvent::ToolCallError { id, message } => vec![HarnessServerEvent::ToolCallError {
            session_id: session_id.clone(),
            id,
            message,
        }],
        HarnessEvent::AskUser {
            question,
            quick_replies,
        } => vec![HarnessServerEvent::AskUser {
            session_id: session_id.clone(),
            question,
            quick_replies,
        }],
        // A command proposal is the harness-side suspension (A6). Carrying it to
        // the wire and rendering the approve/edit composer is the renderer item
        // (A5), so nothing is mapped here yet, exactly like `ThinkingModeChanged`.
        HarnessEvent::CommandProposed { .. } => Vec::new(),
        HarnessEvent::MissionUpdated { mission_id, status } => {
            vec![HarnessServerEvent::MissionUpdated {
                session_id: session_id.clone(),
                mission_id,
                status,
            }]
        }
        HarnessEvent::Done => vec![HarnessServerEvent::Done {
            session_id: session_id.clone(),
        }],
        HarnessEvent::Error(message) => vec![HarnessServerEvent::Error {
            session_id: session_id.clone(),
            message,
        }],
        HarnessEvent::ReasoningStarted { step, mode } => {
            vec![HarnessServerEvent::ReasoningStarted {
                session_id: session_id.clone(),
                step,
                mode,
            }]
        }
        HarnessEvent::ReasoningDelta(delta) => vec![HarnessServerEvent::ReasoningDelta {
            session_id: session_id.clone(),
            delta,
        }],
        HarnessEvent::ReasoningDone {
            step,
            mode,
            content,
            decision,
        } => vec![HarnessServerEvent::ReasoningDone {
            session_id: session_id.clone(),
            step,
            mode,
            content,
            decision,
        }],
        HarnessEvent::ThinkingModeChanged(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use goble_core::llm::{CompletionResponse, MockProvider};

    fn test_harness(reply: &str) -> InternalHarness {
        let provider = MockProvider::new(
            "mock",
            CompletionResponse {
                content: reply.to_string(),
                tool_calls: Vec::new(),
            },
        );
        InternalHarness::new(Store::open_in_memory().unwrap())
            .with_provider("mock")
            .with_model("mock")
            .with_llm(Arc::new(provider))
    }

    #[tokio::test]
    async fn run_streams_mapped_assistant_delta_and_done() {
        let harness = test_harness("hello harness");
        let turn = HarnessTurn::new(
            HarnessId::new("internal"),
            SessionId::new("s1"),
            "say hello",
        );
        let mut run = harness.run(turn, Arc::new(AtomicBool::new(false)));

        let mut seen = Vec::new();
        while let Some(ev) = run.events.next().await {
            seen.push(ev);
        }

        assert!(seen.iter().any(|e| matches!(
            e,
            HarnessServerEvent::AssistantDelta { delta, .. } if delta == "hello harness"
        )));
        assert!(seen.iter().any(|e| matches!(e, HarnessServerEvent::Done { .. })));
    }

    #[test]
    fn open_screen_tool_call_maps_to_handoff() {
        let session_id = SessionId::new("s1");
        let events = map_event(
            &session_id,
            HarnessEvent::ToolCallStarted {
                id: "t1".to_string(),
                name: OPEN_SCREEN_TOOL.to_string(),
                arguments: serde_json::json!({ "host": "vm.example.com", "username": "u", "password": "p" }),
            },
        );
        assert!(events.iter().any(|e| matches!(e, HarnessServerEvent::ToolCallStarted { name, .. } if name == OPEN_SCREEN_TOOL)));
        assert!(events.iter().any(|e| matches!(
            e,
            HarnessServerEvent::ScreenHandoff { config, .. } if config.host == "vm.example.com" && config.username == "u"
        )));
    }

    #[test]
    fn reasoning_events_are_carried_to_the_wire() {
        let session_id = SessionId::new("s1");

        let started = map_event(
            &session_id,
            HarnessEvent::ReasoningStarted {
                step: 2,
                mode: "contemplating".to_string(),
            },
        );
        assert!(matches!(
            started.as_slice(),
            [HarnessServerEvent::ReasoningStarted { step: 2, mode, .. }] if mode == "contemplating"
        ));

        let delta = map_event(
            &session_id,
            HarnessEvent::ReasoningDelta("weighing".to_string()),
        );
        assert!(matches!(
            delta.as_slice(),
            [HarnessServerEvent::ReasoningDelta { delta, .. }] if delta == "weighing"
        ));

        let done = map_event(
            &session_id,
            HarnessEvent::ReasoningDone {
                step: 2,
                mode: "contemplating".to_string(),
                content: "weighing options".to_string(),
                decision: "\"execute\"".to_string(),
            },
        );
        assert!(matches!(
            done.as_slice(),
            [HarnessServerEvent::ReasoningDone { step: 2, content, .. }] if content == "weighing options"
        ));

        // `ThinkingModeChanged` is the only harness event the wire still drops.
        assert!(map_event(
            &session_id,
            HarnessEvent::ThinkingModeChanged("direct".to_string())
        )
        .is_empty());
    }

    #[test]
    fn capabilities_is_internal() {
        let harness = test_harness("hi");
        assert_eq!(harness.capabilities(), HarnessCapabilities::internal());
    }

    #[test]
    fn id_defaults_to_internal() {
        let harness = test_harness("hi");
        assert_eq!(harness.id(), HarnessId::new("internal"));
    }

    #[test]
    fn with_workspace_dir_forwards_to_inner_harness() {
        let dir = std::path::PathBuf::from("/tmp/goble-ws");
        let harness = test_harness("hi").with_workspace_dir(dir.clone());
        assert_eq!(harness.harness.workspace_dir(), dir.as_path());
    }
}
