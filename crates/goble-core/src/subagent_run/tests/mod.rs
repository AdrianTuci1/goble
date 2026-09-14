//! Unit tests for the child run, one module per surface of a child's life: the
//! ceilings it is charged against ([`budget`]), the stop bit that ends it
//! ([`cancel`]), the registry its record lives in ([`registry`]), the wire
//! events a transition becomes ([`events`]) and the spawn and turn behaviour
//! ([`host`], [`run`]). What the modules drive in common — the scripted and
//! gated providers, the store and workspace fixtures, the registry poll — lives
//! here.

use std::collections::VecDeque;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use futures::Stream;
use tokio::sync::oneshot;

use crate::harness::{
    execute_tool_call, Harness, MockCommandRunner, ToolTurnContext, WebSearchConfig,
    SPAWN_SUBAGENT_TOOL,
};
use crate::llm::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, LlmToolCall, Role,
    TokenUsage,
};
use crate::mcp_manager::McpManager;
use crate::store::Store;
use crate::subagent::{SubAgentBudget, SubAgentId, SubAgentRecord, SubAgentSpec};

use super::cancel::ChildCancel;
use super::host::ChildDeps;
use super::run::run_child_turn;
use super::{SubAgentHost, SubAgentRegistry, DEFAULT_FOREGROUND_CHILD_WAIT};

mod budget;
mod cancel;
mod events;
mod host;
mod registry;
mod run;

/// A stub provider with a scripted answer per call and a fallback once the
/// script runs out — the always-tool-call fallback is what makes a runaway
/// child hit its budget instead of the test hanging. No network.
struct ScriptedProvider {
    steps: std::sync::Mutex<VecDeque<CompletionResponse>>,
    fallback: CompletionResponse,
}

impl ScriptedProvider {
    fn scripted(
        steps: Vec<CompletionResponse>,
        fallback: CompletionResponse,
    ) -> Arc<dyn LlmProvider> {
        Arc::new(Self {
            steps: std::sync::Mutex::new(steps.into()),
            fallback,
        })
    }

    fn loops_forever(fallback: CompletionResponse) -> Arc<dyn LlmProvider> {
        Self::scripted(Vec::new(), fallback)
    }

    fn next_response(&self) -> CompletionResponse {
        self.steps
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone())
    }
}

#[async_trait::async_trait]
impl LlmProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse> {
        Ok(self.next_response())
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        let response = self.next_response();
        let events = vec![
            CompletionStreamEvent::AssistantDelta(response.content),
            CompletionStreamEvent::ToolCalls(response.tool_calls),
            CompletionStreamEvent::Done,
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

fn spawn_args(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "description": "review the diff",
        "prompt": prompt,
        "subagent_type": "reviewer",
        "run_in_background": false,
    })
}

fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> LlmToolCall {
    LlmToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
    }
}

fn completion(content: &str, tool_calls: Vec<LlmToolCall>) -> CompletionResponse {
    CompletionResponse {
        content: content.to_string(),
        tool_calls,
        usage: None,
    }
}

/// A completion that carries what the provider reported the call cost, the way
/// a real provider's non-streaming response does.
fn completion_reporting(
    content: &str,
    tool_calls: Vec<LlmToolCall>,
    usage: TokenUsage,
) -> CompletionResponse {
    CompletionResponse {
        content: content.to_string(),
        tool_calls,
        usage: Some(usage),
    }
}

fn parent_chat(store: &Store) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    store
        .insert_chat(&id, "parent", None, None, &now, &now)
        .unwrap();
    id
}

fn child_of(store: &Store, parent_chat: &str) -> String {
    store
        .list_chats()
        .unwrap()
        .into_iter()
        .find(|(id, ..)| {
            id != parent_chat
                && store.get_chat_parent(id).unwrap().as_deref() == Some(parent_chat)
        })
        .map(|(id, ..)| id)
        .expect("a child chats row carrying the parent id")
}

fn roles(store: &Store, chat_id: &str) -> Vec<(String, String)> {
    store
        .list_chat_messages(chat_id)
        .unwrap()
        .into_iter()
        .map(|(_, role, content, _, _)| (role, content))
        .collect()
}

fn spec_for(id: &str, cwd: &Path, budget: SubAgentBudget) -> SubAgentSpec {
    SubAgentSpec {
        id: SubAgentId(id.to_string()),
        parent_chat_id: "parent-chat".to_string(),
        parent_call_id: "call-1".to_string(),
        description: "review the diff".to_string(),
        subagent_type: "reviewer".to_string(),
        prompt: "review it".to_string(),
        cwd: cwd.to_path_buf(),
        run_in_background: false,
        depth: 1,
        budget,
    }
}

/// An always-tool-calling answer: what a runaway child would keep
/// producing, so only the budget can stop it.
fn credentials_call_forever() -> CompletionResponse {
    completion(
        "",
        vec![tool_call("tc1", "credentials", serde_json::json!({}))],
    )
}

async fn run_direct(
    store: &Store,
    workspace: &Path,
    llm: Arc<dyn LlmProvider>,
    spec: SubAgentSpec,
) -> SubAgentRecord {
    let now = Utc::now().to_rfc3339();
    store
        .insert_subagent_chat(
            &spec.id.0,
            &spec.description,
            &spec.parent_chat_id,
            Some("mock"),
            Some("model"),
            &now,
            &now,
        )
        .unwrap();
    std::fs::create_dir_all(&spec.cwd).unwrap();
    let host = host_with_wait(store, workspace, llm, DEFAULT_FOREGROUND_CHILD_WAIT);
    let deps = ChildDeps {
        host,
        provider: "mock".to_string(),
        model: "model".to_string(),
        web_search: WebSearchConfig::default(),
        cancel: ChildCancel::new(Arc::new(AtomicBool::new(false))),
    };
    let mut record = SubAgentRecord::new(spec);
    run_child_turn(&mut record, &deps).await;
    record
}

/// The host `Harness::subagent_host` builds for a turn: the same handles, a
/// registry of its own, a parent bit nobody has flipped and the default
/// foreground budget.
fn host_for(store: &Store, workspace: &Path, llm: Arc<dyn LlmProvider>) -> SubAgentHost {
    host_with_wait(store, workspace, llm, DEFAULT_FOREGROUND_CHILD_WAIT)
}

fn host_with_wait(
    store: &Store,
    workspace: &Path,
    llm: Arc<dyn LlmProvider>,
    foreground_wait: Duration,
) -> SubAgentHost {
    SubAgentHost {
        store: store.clone(),
        runner: Arc::new(MockCommandRunner),
        llm,
        deploy_sender: None,
        mcp_manager: McpManager::new(),
        docs_dir: workspace.to_path_buf(),
        registry: SubAgentRegistry::default(),
        parent_cancel: Arc::new(AtomicBool::new(false)),
        foreground_wait,
    }
}

/// Each child's model call parks on a gate of its own, chosen by the prompt
/// in the request so the mapping never depends on which child the runtime
/// polls first. The answer the test sends through the gate is what the child's
/// call returns, so a child is provably mid-run with nothing slept on. The
/// parent's turn is answered from a script through the stream the harness loop
/// uses, so a test can drive a whole turn.
struct GateProvider {
    gates: std::sync::Mutex<Vec<(String, oneshot::Receiver<String>)>>,
    parent_turns: std::sync::Mutex<VecDeque<CompletionResponse>>,
}

impl GateProvider {
    /// One gate per child, in the order the prompts are given; the senders
    /// open them.
    fn gated(prompts: &[&str]) -> (Arc<dyn LlmProvider>, Vec<oneshot::Sender<String>>) {
        Self::gated_with(prompts, Vec::new())
    }

    fn gated_with(
        prompts: &[&str],
        parent_turns: Vec<CompletionResponse>,
    ) -> (Arc<dyn LlmProvider>, Vec<oneshot::Sender<String>>) {
        let (senders, receivers): (Vec<_>, Vec<_>) =
            prompts.iter().map(|_| oneshot::channel::<String>()).unzip();
        let gates = prompts
            .iter()
            .map(|prompt| prompt.to_string())
            .zip(receivers)
            .collect();
        (
            Arc::new(Self {
                gates: std::sync::Mutex::new(gates),
                parent_turns: std::sync::Mutex::new(parent_turns.into()),
            }),
            senders,
        )
    }

    fn take_gate(&self, prompt: &str) -> oneshot::Receiver<String> {
        let mut gates = self.gates.lock().unwrap();
        let index = gates
            .iter()
            .position(|(needle, _)| needle == prompt)
            .expect("a gate for this child's prompt");
        gates.swap_remove(index).1
    }
}

#[async_trait::async_trait]
impl LlmProvider for GateProvider {
    fn name(&self) -> &str {
        "gated"
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let prompt = request
            .messages
            .iter()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let gate = self.take_gate(&prompt);
        let text = gate.await.unwrap_or_default();
        Ok(completion(&text, Vec::new()))
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        let response = self
            .parent_turns
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| completion("", Vec::new()));
        let events = vec![
            CompletionStreamEvent::AssistantDelta(response.content),
            CompletionStreamEvent::ToolCalls(response.tool_calls),
            CompletionStreamEvent::Done,
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

/// A provider that unwinds on the child's first call.
struct PanickingProvider;

#[async_trait::async_trait]
impl LlmProvider for PanickingProvider {
    fn name(&self) -> &str {
        "panicking"
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse> {
        panic!("child exploded")
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        Err(anyhow::anyhow!("this stub only answers a child's call"))
    }
}

/// Spawn one child through the harness tool and hand back what the call
/// returned, so every test drives the same path the model drives.
async fn spawn_through_tool(
    host: &SubAgentHost,
    store: &Store,
    workspace: &Path,
    parent_chat: &str,
    args: serde_json::Value,
) -> Result<String> {
    let turn = ToolTurnContext {
        chat_id: parent_chat,
        provider: "mock",
        model: "model",
        depth: 0,
        host,
    };
    execute_tool_call(
        store,
        &MockCommandRunner,
        None,
        &McpManager::new(),
        workspace,
        workspace,
        &tool_call("tc-spawn", SPAWN_SUBAGENT_TOOL, args),
        &WebSearchConfig::default(),
        &turn,
    )
    .await
}

fn background_args(prompt: &str) -> serde_json::Value {
    let mut args = spawn_args(prompt);
    args["run_in_background"] = serde_json::json!(true);
    args
}

/// Poll the registry until the child is terminal. Bounded on purpose: a child
/// that never ends is the failure under test, and a test must not hang on it.
async fn wait_until_terminal(host: &SubAgentHost, id: &SubAgentId) -> SubAgentRecord {
    for _ in 0..2_000 {
        let record = host
            .registry
            .snapshot(id)
            .expect("the spawn registered the child's record");
        if record.status.is_terminal() {
            return record;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("sub-agent {id} never reached a terminal status");
}

/// Let the runtime run everything a released child task still has to do.
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}

fn harness_on(store: Store, workspace: &Path, llm: Arc<dyn LlmProvider>) -> Harness {
    Harness::new(store)
        .with_llm(llm)
        .with_workspace_dir(workspace)
}

fn turn_with<'a>(host: &'a SubAgentHost, chat_id: &'a str, depth: u32) -> ToolTurnContext<'a> {
    ToolTurnContext {
        chat_id,
        provider: "mock",
        model: "model",
        depth,
        host,
    }
}
