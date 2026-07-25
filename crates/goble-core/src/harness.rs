use std::collections::HashSet;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use chrono::Utc;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, AgentSpec, Trigger};
use crate::llm::{CompletionRequest, CompletionResponse, LlmProvider, LlmToolCall, Role, ToolDefinition};
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::worker::WorkerId;
use crate::workflow::{Workflow, WorkflowStep};

/// A command that the harness can execute directly from a tool call.
#[async_trait::async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &str, args: &[String]) -> Result<String>;
}

/// A no-op / safe command runner for tests and environments where shell execution
/// is not available.
pub struct MockCommandRunner;

#[async_trait::async_trait]
impl CommandRunner for MockCommandRunner {
    async fn run(&self, command: &str, args: &[String]) -> Result<String> {
        Ok(format!("mock ran `{command}` with args {args:?}"))
    }
}

/// A sandboxed shell runner with an allowlist of commands and a per-command timeout.
pub struct SandboxedCommandRunner {
    allowed_commands: HashSet<String>,
    timeout_seconds: u64,
    working_dir: PathBuf,
}

impl SandboxedCommandRunner {
    pub fn new(allowed: impl IntoIterator<Item = impl Into<String>>, timeout_seconds: u64, working_dir: PathBuf) -> Self {
        Self {
            allowed_commands: allowed.into_iter().map(Into::into).collect(),
            timeout_seconds,
            working_dir,
        }
    }

    pub fn default_tools() -> Self {
        Self::new(
            ["echo", "cat", "ls", "pwd", "git", "cargo", "npm", "node", "python3", "rustc"],
            60,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    }
}

#[async_trait::async_trait]
impl CommandRunner for SandboxedCommandRunner {
    async fn run(&self, command: &str, args: &[String]) -> Result<String> {
        if !self.allowed_commands.contains(command) {
            anyhow::bail!("command `{command}` is not in the allowed list");
        }
        let output = tokio::time::timeout(
            Duration::from_secs(self.timeout_seconds),
            tokio::process::Command::new(command)
                .args(args)
                .current_dir(&self.working_dir)
                .output(),
        )
        .await
        .context("command timed out")?
        .context("failed to execute command")?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            anyhow::bail!("command failed: {stderr}");
        }
        Ok(format!("{stdout}{stderr}").trim().to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: Option<String>,
}

/// An event emitted by the harness while processing a user turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum HarnessEvent {
    AssistantDelta(String),
    ToolCallStarted { id: String, name: String, arguments: serde_json::Value },
    ToolCallFinished { id: String, result: String },
    ToolCallError { id: String, message: String },
    Done,
    Error(String),
}

pub struct Harness {
    store: Store,
    runner: Arc<dyn CommandRunner>,
    llm: Arc<dyn LlmProvider>,
    deploy_sender: Option<Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>>,
}

impl Harness {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            runner: Arc::new(MockCommandRunner),
            llm: Arc::new(crate::llm::MockProvider::new(
                "mock",
                CompletionResponse {
                    content: "I can help with that.".to_string(),
                    tool_calls: Vec::new(),
                },
            )),
            deploy_sender: None,
        }
    }

    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = llm;
        self
    }

    pub fn with_deploy_sender<F>(mut self, sender: F) -> Self
    where
        F: Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync + 'static,
    {
        self.deploy_sender = Some(Arc::new(sender));
        self
    }

    pub fn list_tools(&self) -> Vec<ToolSchema> {
        harness_tool_definitions()
            .into_iter()
            .map(|t| ToolSchema {
                name: t.name,
                description: t.description,
                parameters: t.parameters,
            })
            .collect()
    }

    /// Run one conversational turn. Emits streaming events.
    pub fn run_turn(
        &self,
        chat_id: &str,
        prompt: &str,
        provider: &str,
        model: &str,
    ) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
        let store = self.store.clone();
        let runner = Arc::clone(&self.runner);
        let llm = Arc::clone(&self.llm);
        let deploy_sender = self.deploy_sender.clone();
        let chat_id = chat_id.to_string();
        let prompt = prompt.to_string();
        let provider = provider.to_string();
        let model = model.to_string();

        let stream = async_stream::stream! {
            let now = Utc::now().to_rfc3339();
            if let Err(e) = store.insert_chat_message(
                &uuid::Uuid::new_v4().to_string(),
                &chat_id,
                "user",
                &prompt,
                None,
                &now,
            ) {
                yield HarnessEvent::Error(e.to_string());
                return;
            }

            let history = match store.list_chat_messages(&chat_id) {
                Ok(rows) => rows
                    .into_iter()
                    .map(|(_, role, content, tool_calls, _)| crate::llm::Message {
                        role: match role.as_str() {
                            "system" => Role::System,
                            "assistant" => Role::Assistant,
                            "tool" => Role::Tool,
                            _ => Role::User,
                        },
                        content,
                        tool_calls: tool_calls.and_then(|t| serde_json::from_str(&t).ok()),
                    })
                    .collect::<Vec<_>>(),
                Err(e) => {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
            };

            let tools = harness_tool_definitions();
            let request = CompletionRequest::new(provider, model)
                .with_system(HARNESS_SYSTEM_PROMPT)
                .with_tools(tools)
                .with_messages(history);

            let mut stream = match llm.complete_stream(request).await {
                Ok(s) => s,
                Err(e) => {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
            };

            let mut assistant_content = String::new();
            let mut tool_calls = Vec::new();
            while let Some(event) = stream.next().await {
                match event {
                    crate::llm::CompletionStreamEvent::AssistantDelta(delta) => {
                        assistant_content.push_str(&delta);
                        yield HarnessEvent::AssistantDelta(delta);
                    }
                    crate::llm::CompletionStreamEvent::ToolCalls(calls) => {
                        tool_calls = calls;
                        let tool_calls_json = serde_json::to_string(&tool_calls).unwrap_or_default();
                        if !tool_calls.is_empty() {
                            if let Err(e) = store.insert_chat_message(
                                &uuid::Uuid::new_v4().to_string(),
                                &chat_id,
                                "tool",
                                &tool_calls_json,
                                Some(&tool_calls_json),
                                &Utc::now().to_rfc3339(),
                            ) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
                        }
                    }
                    crate::llm::CompletionStreamEvent::Done => break,
                    crate::llm::CompletionStreamEvent::Error(message) => {
                        yield HarnessEvent::Error(message);
                        return;
                    }
                }
            }

        if !assistant_content.is_empty() {
            if let Err(e) = store.insert_chat_message(
                &uuid::Uuid::new_v4().to_string(),
                &chat_id,
                "assistant",
                &assistant_content,
                None,
                &Utc::now().to_rfc3339(),
            ) {
                yield HarnessEvent::Error(e.to_string());
                return;
            }
        }

        for call in &tool_calls {
            yield HarnessEvent::ToolCallStarted {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            };

            let sender_ref = deploy_sender.as_ref().map(|f| arc_to_sender_ref(f));
            let result = execute_tool_call(&store, &*runner, sender_ref, &call).await;
            match result {
                Ok(value) => {
                    let tool_result_text = format!("{}\n{}", call.id, value);
                    if let Err(e) = store.insert_chat_message(
                        &uuid::Uuid::new_v4().to_string(),
                        &chat_id,
                        "tool",
                        &tool_result_text,
                        None,
                        &Utc::now().to_rfc3339(),
                    ) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                    yield HarnessEvent::ToolCallFinished { id: call.id.clone(), result: value };
                }
                Err(e) => {
                    let error_text = format!("{}\nERROR: {}", call.id, e);
                    if let Err(e2) = store.insert_chat_message(
                        &uuid::Uuid::new_v4().to_string(),
                        &chat_id,
                        "tool",
                        &error_text,
                        None,
                        &Utc::now().to_rfc3339(),
                    ) {
                        yield HarnessEvent::Error(e2.to_string());
                        return;
                    }
                    yield HarnessEvent::ToolCallError { id: call.id.clone(), message: e.to_string() };
                }
            }
        }

        yield HarnessEvent::Done;
        };

        Box::pin(stream)
    }
}

const HARNESS_SYSTEM_PROMPT: &str = r#"You are an assistant that controls a local Goble agent environment.
You can create/update agents, workflows and teams, run shell commands, read/write files, search the store, deploy agents/workflows to workers, schedule workflows, and check execution status.
Only call a tool when the user explicitly asks for an action you can perform with a tool.
If no tool is needed, reply conversationally.
When reading or writing files, paths must be inside the workspace directory unless the user explicitly provides an absolute path."#;

fn harness_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_agent".to_string(),
            description: "Create a new agent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "prompt": { "type": "string" },
                    "tools": { "type": "array", "items": { "type": "string" } },
                    "mcp_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id", "name", "prompt"]
            }),
        },
        ToolDefinition {
            name: "update_agent".to_string(),
            description: "Update an existing agent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "prompt": { "type": "string" },
                    "tools": { "type": "array", "items": { "type": "string" } },
                    "mcp_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "create_workflow".to_string(),
            description: "Create a new workflow.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "update_workflow".to_string(),
            description: "Update an existing workflow by replacing its definition.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "description": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "create_team".to_string(),
            description: "Create a new team of agents.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "metadata": { "type": "object" },
                    "agent_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id", "name"]
            }),
        },
        ToolDefinition {
            name: "update_team".to_string(),
            description: "Update a team by replacing its metadata and members.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "metadata": { "type": "object" },
                    "agent_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_string(),
            description: "Run an allowed shell command with a timeout.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["command"]
            }),
        },
        ToolDefinition {
            name: "list_entities".to_string(),
            description: "List existing agents, workflows, teams or workers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_type": { "type": "string", "enum": ["agents", "workflows", "teams", "workers"] }
                },
                "required": ["entity_type"]
            }),
        },
        ToolDefinition {
            name: "search_store".to_string(),
            description: "Search agents, workflows and teams by name.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "entity_types": { "type": "array", "items": { "type": "string", "enum": ["agents", "workflows", "teams"] } }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "deploy_agent".to_string(),
            description: "Deploy an agent to a worker.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "worker_id": { "type": "string" }
                },
                "required": ["agent_id", "worker_id"]
            }),
        },
        ToolDefinition {
            name: "deploy_workflow".to_string(),
            description: "Deploy a workflow to a worker.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string" },
                    "worker_id": { "type": "string" }
                },
                "required": ["workflow_id", "worker_id"]
            }),
        },
        ToolDefinition {
            name: "schedule_workflow".to_string(),
            description: "Schedule a workflow to run on a trigger.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_id": { "type": "string" },
                    "trigger_type": { "type": "string", "enum": ["cron", "http", "heartbeat"] },
                    "trigger_value": { "type": "string" }
                },
                "required": ["workflow_id", "trigger_type", "trigger_value"]
            }),
        },
        ToolDefinition {
            name: "get_execution_status".to_string(),
            description: "Get the status of an execution by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "execution_id": { "type": "string" }
                },
                "required": ["execution_id"]
            }),
        },
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write a file inside the workspace. Creates parent directories.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "delete_agent".to_string(),
            description: "Delete an agent by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "delete_workflow".to_string(),
            description: "Delete a workflow by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "delete_team".to_string(),
            description: "Delete a team by id.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "rename_file".to_string(),
            description: "Rename or move a file inside the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string" },
                    "to": { "type": "string" }
                },
                "required": ["from", "to"]
            }),
        },
        ToolDefinition {
            name: "delete_file".to_string(),
            description: "Delete a file inside the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "git_status".to_string(),
            description: "Run git status in the workspace.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "git_diff".to_string(),
            description: "Run git diff in the workspace. Optionally pass a path.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "git_commit".to_string(),
            description: "Stage files and create a git commit.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "files": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["message"]
            }),
        },
        ToolDefinition {
            name: "codebase_search".to_string(),
            description: "Search for a regex inside workspace files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "install_mcp_server".to_string(),
            description: "Register an MCP server in the store.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "source": { "type": "string", "enum": ["github", "npm", "local", "url"] },
                    "source_value": { "type": "string" },
                    "manifest": { "type": "string" }
                },
                "required": ["id", "name", "source", "source_value"]
            }),
        },
        ToolDefinition {
            name: "list_mcp_servers".to_string(),
            description: "List registered MCP servers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web using DuckDuckGo HTML. Returns up to 10 results with title, snippet and URL.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "read_url".to_string(),
            description: "Fetch a URL and extract the main text content.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
        },
        ToolDefinition {
            name: "execute_python_code".to_string(),
            description: "Execute Python code in a temporary file and return stdout/stderr.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string" }
                },
                "required": ["code"]
            }),
        },
        ToolDefinition {
            name: "run_agent".to_string(),
            description: "Run a local agent once and return its prompt output.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "input": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Replace old text with new text in a workspace file.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_text": { "type": "string" },
                    "new_text": { "type": "string" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        },
    ]
}


fn arc_to_sender_ref(arc: &Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>) -> &(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync) {
    &**arc
}
async fn execute_tool_call(
    store: &Store,
    runner: &dyn CommandRunner,
    deploy_sender: Option<&(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync)>,
    call: &LlmToolCall,
) -> Result<String> {
    match call.name.as_str() {
        "create_agent" => create_agent(store, &call.arguments),
        "update_agent" => update_agent(store, &call.arguments),
        "create_workflow" => create_workflow(store, &call.arguments),
        "update_workflow" => update_workflow(store, &call.arguments),
        "create_team" => create_team(store, &call.arguments),
        "update_team" => update_team(store, &call.arguments),
        "run_command" => run_command(runner, &call.arguments).await,
        "list_entities" => list_entities(store, &call.arguments),
        "search_store" => search_store(store, &call.arguments),
        "deploy_agent" => deploy_agent(store, deploy_sender, &call.arguments),
        "deploy_workflow" => deploy_workflow(store, deploy_sender, &call.arguments),
        "schedule_workflow" => schedule_workflow(store, &call.arguments),
        "get_execution_status" => get_execution_status(store, &call.arguments),
        "delete_agent" => delete_agent(store, &call.arguments),
        "delete_workflow" => delete_workflow(store, &call.arguments),
        "delete_team" => delete_team(store, &call.arguments),
        "rename_file" => rename_file(&call.arguments),
        "delete_file" => delete_file(&call.arguments),
        "git_status" => git_status(runner, &call.arguments).await,
        "git_diff" => git_diff(runner, &call.arguments).await,
        "git_commit" => git_commit(runner, &call.arguments).await,
        "codebase_search" => codebase_search(&call.arguments),
        "install_mcp_server" => install_mcp_server(store, &call.arguments),
        "list_mcp_servers" => list_mcp_servers(store),
        "run_agent" => run_agent(store, &call.arguments),
        "read_file" => read_file(&call.arguments),
        "write_file" => write_file(&call.arguments),
        "edit_file" => edit_file(&call.arguments),
        "web_search" => web_search(&call.arguments).await,
        "read_url" => read_url(&call.arguments).await,
        "execute_python_code" => execute_python_code(&call.arguments).await,
        _ => anyhow::bail!("unknown tool {}", call.name),
    }
}

fn create_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = args["name"].as_str().unwrap_or(&id).to_string();
    let description = args["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let prompt = args["prompt"]
        .as_str()
        .context("prompt is required")?
        .to_string();
    let tools = args["tools"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mcp_ids = args["mcp_ids"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    let spec = AgentSpec {
        id: AgentId(id.clone()),
        name,
        description,
        prompt,
        tools,
        triggers: vec![Trigger::Manual],
        mcp_ids,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let spec_json = serde_json::to_string(&spec)?;
    store.insert_agent(&id, &spec.name, &spec_json, &now, &now)?;
    Ok(format!("agent {id} created"))
}

fn update_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let existing = store
        .get_agent(id)?
        .context("agent not found")?;
    let mut spec: AgentSpec = serde_json::from_str(&existing.2)?;
    if let Some(name) = args["name"].as_str() {
        spec.name = name.to_string();
    }
    if let Some(description) = args["description"].as_str() {
        spec.description = description.to_string();
    }
    if let Some(prompt) = args["prompt"].as_str() {
        spec.prompt = prompt.to_string();
    }
    if let Some(tools) = args["tools"].as_array() {
        spec.tools = tools.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }
    if let Some(mcp_ids) = args["mcp_ids"].as_array() {
        spec.mcp_ids = mcp_ids.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }
    spec.updated_at = Utc::now().to_rfc3339();
    let spec_json = serde_json::to_string(&spec)?;
    store.insert_agent(id, &spec.name, &spec_json, &spec.created_at, &spec.updated_at)?;
    Ok(format!("agent {id} updated"))
}

fn create_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let name = args["name"].as_str().context("name is required")?;
    let description = args["description"].as_str().unwrap_or_default();
    let steps = parse_workflow_steps(&args["steps"]);
    let workflow = Workflow::new(name, description).with_steps(steps);
    let spec_json = serde_json::to_string(&workflow)?;
    let trigger_str = serde_json::to_string(&workflow.trigger)?;
    let now = workflow.created_at.clone();
    store.insert_workflow(
        &workflow.id.to_string(),
        &workflow.name,
        &workflow.description,
        &spec_json,
        &trigger_str,
        workflow.enabled,
        &now,
        &now,
    )?;
    Ok(format!("workflow {} created", workflow.id))
}

fn update_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let rows = store.list_workflows()?;
    let existing = rows
        .into_iter()
        .find(|(i, _, _, _, _, _, _, _)| i == id)
        .context("workflow not found")?;
    let mut workflow: Workflow = serde_json::from_str(&existing.3)?;
    if let Some(name) = args["name"].as_str() {
        workflow.name = name.to_string();
    }
    if let Some(description) = args["description"].as_str() {
        workflow.description = description.to_string();
    }
    if args["steps"].is_array() {
        workflow.steps = parse_workflow_steps(&args["steps"]);
    }
    workflow.updated_at = Utc::now().to_rfc3339();
    let spec_json = serde_json::to_string(&workflow)?;
    let trigger_str = serde_json::to_string(&workflow.trigger)?;
    store.insert_workflow(
        &workflow.id.to_string(),
        &workflow.name,
        &workflow.description,
        &spec_json,
        &trigger_str,
        workflow.enabled,
        &workflow.created_at,
        &workflow.updated_at,
    )?;
    Ok(format!("workflow {id} updated"))
}

fn parse_workflow_steps(value: &serde_json::Value) -> Vec<WorkflowStep> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| WorkflowStep {
                    id: v["id"].as_str().map(|s| s.to_string()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    name: v["name"].as_str().unwrap_or("step").to_string(),
                    agent_id: AgentId(v["agent_id"].as_str().unwrap_or_default().to_string()),
                    input_template: v["input_template"].as_str().unwrap_or("").to_string(),
                    depends_on: v["depends_on"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn create_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = args["name"].as_str().context("name is required")?;
    let metadata = args["metadata"].as_object().cloned().unwrap_or_default();
    let agent_ids: Vec<String> = args["agent_ids"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let metadata_json = serde_json::to_string(&metadata)?;
    let now = Utc::now().to_rfc3339();
    store.insert_team(&id, name, &metadata_json, &now)?;
    for agent_id in &agent_ids {
        store.insert_team_member(&id, agent_id)?;
    }
    Ok(format!("team {id} created/updated with {} members", agent_ids.len()))
}

fn update_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    create_team(store, args)
}

async fn run_command(runner: &dyn CommandRunner, args: &serde_json::Value) -> Result<String> {
    let command = args["command"].as_str().unwrap_or_default();
    let cmd_args: Vec<String> = args["args"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    runner.run(command, &cmd_args).await
}

fn list_entities(store: &Store, args: &serde_json::Value) -> Result<String> {
    let entity_type = args["entity_type"].as_str().unwrap_or("agents");
    match entity_type {
        "agents" => {
            let rows = store.list_agents()?;
            Ok(format!("agents: {:?}", rows.into_iter().map(|(id, name, _, _, _)| (id, name)).collect::<Vec<_>>()))
        }
        "workflows" => {
            let rows = store.list_workflows()?;
            Ok(format!("workflows: {:?}", rows.into_iter().map(|(id, name, _, _, _, _, _, _)| (id, name)).collect::<Vec<_>>()))
        }
        "teams" => {
            let rows = store.list_teams()?;
            Ok(format!("teams: {:?}", rows.into_iter().map(|(id, name, _, _)| (id, name)).collect::<Vec<_>>()))
        }
        "workers" => {
            let rows = store.list_workers()?;
            Ok(format!("workers: {:?}", rows.into_iter().map(|(id, name, host, status, _, _, _, _)| (id, name, host, status)).collect::<Vec<_>>()))
        }
        _ => Ok(format!("unknown entity type {entity_type}")),
    }
}

fn search_store(store: &Store, args: &serde_json::Value) -> Result<String> {
    let query = args["query"].as_str().unwrap_or("").to_lowercase();
    let types: HashSet<String> = args["entity_types"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| ["agents", "workflows", "teams"].iter().map(|s| s.to_string()).collect());

    let mut results = Vec::new();
    if types.contains("agents") {
        for (id, name, _, _, _) in store.list_agents()? {
            if id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query) {
                results.push(("agent", id, name));
            }
        }
    }
    if types.contains("workflows") {
        for (id, name, _, _, _, _, _, _) in store.list_workflows()? {
            if id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query) {
                results.push(("workflow", id, name));
            }
        }
    }
    if types.contains("teams") {
        for (id, name, _, _) in store.list_teams()? {
            if id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query) {
                results.push(("team", id, name));
            }
        }
    }
    Ok(format!("search results: {:?}", results))
}

fn deploy_agent(
    store: &Store,
    deploy_sender: Option<&(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync)>,
    args: &serde_json::Value,
) -> Result<String> {
    let agent_id = args["agent_id"].as_str().context("agent_id is required")?;
    let worker_id = args["worker_id"].as_str().context("worker_id is required")?;
    let (_, _, spec_json, _, _) = store
        .get_agent(agent_id)?
        .context("agent not found")?;
    let spec: AgentSpec = serde_json::from_str(&spec_json)?;
    let worker = WorkerId(worker_id.to_string());
    let message = DesktopMessage::UpdateAgent {
        agent_id: AgentId(agent_id.to_string()),
        spec,
    };
    if let Some(sender) = deploy_sender {
        sender(&worker, message)?;
        Ok(format!("deployed agent {agent_id} to worker {worker_id}"))
    } else {
        Ok(format!("no deploy channel configured; would deploy agent {agent_id} to worker {worker_id}"))
    }
}

fn deploy_workflow(
    store: &Store,
    deploy_sender: Option<&(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync)>,
    args: &serde_json::Value,
) -> Result<String> {
    let workflow_id = args["workflow_id"].as_str().context("workflow_id is required")?;
    let worker_id = args["worker_id"].as_str().context("worker_id is required")?;
    let rows = store.list_workflows()?;
    let existing = rows
        .into_iter()
        .find(|(id, _, _, _, _, _, _, _)| id == workflow_id)
        .context("workflow not found")?;
    let workflow: Workflow = serde_json::from_str(&existing.3)?;
    let worker = WorkerId(worker_id.to_string());
    let message = DesktopMessage::RunAgent {
        trace_id: uuid::Uuid::new_v4().to_string(),
        agent_id: AgentId(workflow.id.to_string()),
        spec: AgentSpec {
            id: AgentId(workflow.id.to_string()),
            name: workflow.name.clone(),
            description: workflow.description.clone(),
            prompt: serde_json::to_string(&workflow.steps)?,
            tools: Vec::new(),
            triggers: vec![workflow.trigger.clone()],
            mcp_ids: Vec::new(),
            created_at: workflow.created_at.clone(),
            updated_at: workflow.updated_at.clone(),
        },
    };
    if let Some(sender) = deploy_sender {
        sender(&worker, message)?;
        Ok(format!("deployed workflow {workflow_id} to worker {worker_id}"))
    } else {
        Ok(format!("no deploy channel configured; would deploy workflow {workflow_id} to worker {worker_id}"))
    }
}

fn schedule_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let workflow_id = args["workflow_id"].as_str().context("workflow_id is required")?;
    let trigger_type = args["trigger_type"].as_str().context("trigger_type is required")?;
    let trigger_value = args["trigger_value"].as_str().context("trigger_value is required")?;
    let rows = store.list_workflows()?;
    let existing = rows
        .into_iter()
        .find(|(id, _, _, _, _, _, _, _)| id == workflow_id)
        .context("workflow not found")?;
    let mut workflow: Workflow = serde_json::from_str(&existing.3)?;
    workflow.trigger = match trigger_type {
        "cron" => Trigger::Cron { expression: trigger_value.to_string() },
        "http" => Trigger::Http { path: trigger_value.to_string() },
        "heartbeat" => {
            let interval = trigger_value.parse::<u64>().context("heartbeat interval must be a number")?;
            Trigger::Heartbeat { interval_seconds: interval }
        }
        _ => anyhow::bail!("unknown trigger type {trigger_type}"),
    };
    workflow.updated_at = Utc::now().to_rfc3339();
    let spec_json = serde_json::to_string(&workflow)?;
    let trigger_str = serde_json::to_string(&workflow.trigger)?;
    store.insert_workflow(
        &workflow.id.to_string(),
        &workflow.name,
        &workflow.description,
        &spec_json,
        &trigger_str,
        workflow.enabled,
        &workflow.created_at,
        &workflow.updated_at,
    )?;
    Ok(format!("workflow {workflow_id} scheduled with {trigger_type}={trigger_value}"))
}

fn get_execution_status(store: &Store, args: &serde_json::Value) -> Result<String> {
    let execution_id = args["execution_id"].as_str().context("execution_id is required")?;
    let rows = store.list_executions()?;
    let exec = rows
        .into_iter()
        .find(|(id, _, _, _, _, _, _)| id == execution_id)
        .context("execution not found")?;
    Ok(format!(
        "execution {execution_id}: status={}, agent={}, worker={}, started_at={}",
        exec.3,
        exec.1.as_deref().unwrap_or("-"),
        exec.2.as_deref().unwrap_or("-"),
        exec.5
    ))
}

fn read_file(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let path = resolve_path(path)?;
    std::fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))
}

fn write_file(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let content = args["content"].as_str().context("content is required")?;
    let path = resolve_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content).with_context(|| format!("failed to write {path:?}"))?;
    Ok(format!("wrote {path:?}"))
}

fn edit_file(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let old_text = args["old_text"].as_str().context("old_text is required")?;
    let new_text = args["new_text"].as_str().context("new_text is required")?;
    let path = resolve_path(path)?;
    let content = std::fs::read_to_string(&path).with_context(|| format!("failed to read {path:?}"))?;
    let replaced = content.replacen(old_text, new_text, 1);
    if replaced == content {
        anyhow::bail!("old_text not found in file");
    }
    std::fs::write(&path, replaced).with_context(|| format!("failed to write {path:?}"))?;
    Ok(format!("edited {path:?}"))
}


fn delete_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    store.delete_agent(id)?;
    Ok(format!("agent {id} deleted"))
}

fn delete_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    store.delete_workflow(id)?;
    Ok(format!("workflow {id} deleted"))
}

fn delete_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    store.delete_team(id)?;
    
    Ok(format!("team {id} deleted"))
}

fn rename_file(args: &serde_json::Value) -> Result<String> {
    let from = args["from"].as_str().context("from is required")?;
    let to = args["to"].as_str().context("to is required")?;
    let from_path = resolve_path(from)?;
    let to_path = resolve_path(to)?;
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&from_path, &to_path).with_context(|| format!("failed to rename {from_path:?} to {to_path:?}"))?;
    Ok(format!("renamed {from_path:?} to {to_path:?}"))
}

fn delete_file(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().context("path is required")?;
    let path = resolve_path(path)?;
    std::fs::remove_file(&path).with_context(|| format!("failed to delete {path:?}"))?;
    Ok(format!("deleted {path:?}"))
}

async fn git_status(runner: &dyn CommandRunner, _args: &serde_json::Value) -> Result<String> {
    runner.run("git", &["status".to_string(), "--short".to_string()]).await
}

async fn git_diff(runner: &dyn CommandRunner, args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str().unwrap_or_default();
    let mut cmd_args = vec!["diff".to_string()];
    if !path.is_empty() {
        cmd_args.push(path.to_string());
    }
    runner.run("git", &cmd_args).await
}

async fn git_commit(runner: &dyn CommandRunner, args: &serde_json::Value) -> Result<String> {
    let message = args["message"].as_str().context("message is required")?;
    let files: Vec<String> = args["files"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if !files.is_empty() {
        let mut add_args = vec!["add".to_string()];
        add_args.extend(files);
        runner.run("git", &add_args).await?;
    } else {
        runner.run("git", &["add".to_string(), "-A".to_string()]).await?;
    }
    runner.run("git", &["commit".to_string(), "-m".to_string(), message.to_string()]).await
}

fn codebase_search(args: &serde_json::Value) -> Result<String> {
    let pattern = args["pattern"].as_str().context("pattern is required")?;
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let search_path = args["path"].as_str().map(PathBuf::from).unwrap_or_else(|| base.clone());
    let search_path = if search_path.is_absolute() { search_path } else { base.join(search_path) };
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.clone());
    let canonical_search = search_path.canonicalize().unwrap_or_else(|_| search_path.clone());
    if !canonical_search.starts_with(&canonical_base) {
        anyhow::bail!("search path escapes workspace directory");
    }
    let regex = regex_lite::Regex::new(pattern).context("invalid regex")?;
    let mut matches = Vec::new();
    for entry in walkdir::WalkDir::new(&canonical_search).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() { continue; }
        if let Ok(content) = std::fs::read_to_string(path) {
            for (i, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    if matches.len() >= 50 { break; }
                }
            }
        }
        if matches.len() >= 50 { break; }
    }
    Ok(format!("{} matches\n{}", matches.len(), matches.join("\n")))
}

fn install_mcp_server(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let name = args["name"].as_str().context("name is required")?;
    let source = args["source"].as_str().context("source is required")?;
    let source_value = args["source_value"].as_str().context("source_value is required")?;
    let manifest = args["manifest"].as_str().unwrap_or("{}");
    let now = Utc::now().to_rfc3339();
    let metadata = serde_json::json!({ "source": source, "source_value": source_value });
    store.insert_mcp_server(id, name, &metadata.to_string(), manifest, None, &now, &now)?;
    Ok(format!("mcp server {id} installed"))
}

fn list_mcp_servers(store: &Store) -> Result<String> {
    let rows = store.list_mcp_servers()?;
    Ok(format!("mcp servers: {:?}", rows.into_iter().map(|(id, name, _, _, _, _, _)| (id, name)).collect::<Vec<_>>()))
}

fn run_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let agent_id = args["agent_id"].as_str().context("agent_id is required")?;
    let input = args["input"].as_str().unwrap_or("");
    let (_, _, spec_json, _, _) = store
        .get_agent(agent_id)?
        .context("agent not found")?;
    let spec: AgentSpec = serde_json::from_str(&spec_json)?;
    Ok(format!("ran agent {} with input '{}'\nprompt: {}\ntools: {:?}", spec.id, input, spec.prompt, spec.tools))
}

async fn web_search(args: &serde_json::Value) -> Result<String> {
    let query = args["query"].as_str().context("query is required")?;
    let encoded = urlencoding::encode(query);
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (compatible; Goble/1.0)")
        .send()
        .await
        .context("web_search request failed")?
        .text()
        .await
        .context("web_search failed to read body")?;
    let mut results = Vec::new();
    for result_html in resp.split(r#"class="result""#).skip(1) {
        let title = regex_lite::Regex::new(r#"class="result__a"[^>]*>(.*?)</a>"#)
            .ok()
            .and_then(|re| re.captures(result_html))
            .and_then(|c| c.get(1))
            .map(|m| html_unescape(m.as_str()))
            .unwrap_or_default();
        let snippet = regex_lite::Regex::new(r#"class="result__snippet"[^>]*>(.*?)</a>"#)
            .ok()
            .and_then(|re| re.captures(result_html))
            .and_then(|c| c.get(1))
            .map(|m| html_unescape(m.as_str()))
            .unwrap_or_default();
        let href = regex_lite::Regex::new(r#"class="result__a"[^>]*href="([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(result_html))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if title.is_empty() && snippet.is_empty() {
            continue;
        }
        results.push(format!("TITLE: {}\nURL: {}\nSNIPPET: {}\n", title, href, snippet));
        if results.len() >= 10 {
            break;
        }
    }
    Ok(format!("{} results\n{}", results.len(), results.join("\n")))
}

async fn read_url(args: &serde_json::Value) -> Result<String> {
    let url = args["url"].as_str().context("url is required")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let html = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (compatible; Goble/1.0)")
        .send()
        .await
        .context("read_url request failed")?
        .text()
        .await
        .context("read_url failed to read body")?;
    let text = html_to_text(&html);
    Ok(text.chars().take(12000).collect())
}

async fn execute_python_code(args: &serde_json::Value) -> Result<String> {
    let code = args["code"].as_str().context("code is required")?;
    let dir = tempfile::tempdir().context("failed to create temp dir")?;
    let file = dir.path().join("script.py");
    std::fs::write(&file, code).context("failed to write python script")?;
    let output = tokio::process::Command::new("python3")
        .arg(&file)
        .current_dir(&dir)
        .output()
        .await
        .context("failed to execute python3")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(format!(
        "exit_code: {}\nstdout:\n{}\nstderr:\n{}",
        output.status.code().unwrap_or(-1),
        stdout,
        stderr
    ))
}

fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut tag_buffer = String::new();
    for ch in html.chars() {
        if ch == '<' && !in_script {
            in_tag = true;
            tag_buffer.clear();
            continue;
        }
        if ch == '>' && in_tag {
            in_tag = false;
            let tag = tag_buffer.trim().to_lowercase();
            if tag.starts_with("script") || tag.starts_with("style") {
                in_script = true;
            } else if tag.starts_with("/script") || tag.starts_with("/style") {
                in_script = false;
            }
            if text.ends_with('\n') || text.is_empty() {
                continue;
            }
            if tag.starts_with("br") || tag.starts_with("p") || tag.starts_with("div") || tag.starts_with("h") || tag.starts_with("li") || tag.starts_with("tr") {
                text.push('\n');
            }
            continue;
        }
        if in_tag {
            tag_buffer.push(ch);
            continue;
        }
        if !in_script {
            text.push(ch);
        }
    }
    let re = regex_lite::Regex::new(r"\n\s*\n").unwrap();
    re.replace_all(&text, "\n").into_owned()
}

fn html_unescape(s: &str) -> String {
    let mut out = s.to_string();
            let entities = [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&nbsp;", " "),
    ];
    for (enc, dec) in &entities {
        out = out.replace(enc, dec);
    }
    let tag_re = regex_lite::Regex::new(r"<[^>]+>").unwrap();
    tag_re.replace_all(&out, "").into_owned().trim().to_string()
}

fn resolve_path(path: &str) -> Result<PathBuf> {
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let p = PathBuf::from(path);
    let resolved = if p.is_absolute() { p } else { base.join(p) };
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.clone());
    let canonical_resolved = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    if !canonical_resolved.starts_with(&canonical_base) {
        anyhow::bail!("path {path:?} escapes workspace directory");
    }
    Ok(canonical_resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockProvider;
    use futures::StreamExt;

    fn chat(store: &Store) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        store.insert_chat(&id, "test", None, None, &now, &now).unwrap();
        id
    }

    fn harness_with_tool(name: &str, arguments: serde_json::Value) -> (Store, String, Harness) {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc1".to_string(),
                    name: name.to_string(),
                    arguments,
                }],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        (store, chat_id, harness)
    }

    #[tokio::test]
    async fn test_harness_create_agent() {
        let (store, chat_id, harness) = harness_with_tool("create_agent", serde_json::json!({
            "id": "agent-1",
            "name": "Greeter",
            "prompt": "Say hello"
        }));
        let events: Vec<_> = harness.run_turn(&chat_id, "make a greeter", "mock", "mock").collect().await;
        let started = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallStarted { name, .. } if name == "create_agent"));
        assert!(started);
        let agents = store.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].0, "agent-1");
    }

    #[tokio::test]
    async fn test_harness_run_command_mock() {
        let (_store, chat_id, harness) = harness_with_tool("run_command", serde_json::json!({
            "command": "echo",
            "args": ["hi"]
        }));
        let events: Vec<_> = harness.run_turn(&chat_id, "run echo hi", "mock", "mock").collect().await;
        let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("mock ran")));
        assert!(finished);
    }

    #[tokio::test]
    async fn test_harness_unknown_command() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: "ok".to_string(),
                tool_calls: vec![LlmToolCall {
                    id: "tc3".to_string(),
                    name: "not_a_tool".to_string(),
                    arguments: serde_json::json!({}),
                }],
            },
        ));
        let harness = Harness::new(store).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "do it", "mock", "mock").collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallError { .. })));
    }

    #[tokio::test]
    async fn test_harness_create_workflow_and_team() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let now = Utc::now().to_rfc3339();
        let spec_a = AgentSpec {
            id: AgentId("agent-a".to_string()),
            name: "A".to_string(),
            description: "".to_string(),
            prompt: "".to_string(),
            tools: vec![],
            triggers: vec![Trigger::Manual],
            mcp_ids: vec![],
            created_at: now.clone(),
            updated_at: now,
        };
        let spec_json = serde_json::to_string(&spec_a).unwrap();
        store.insert_agent("agent-a", "A", &spec_json, &spec_a.created_at, &spec_a.updated_at).unwrap();
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall {
                        id: "tc4".to_string(),
                        name: "create_workflow".to_string(),
                        arguments: serde_json::json!({
                            "name": "wf1",
                            "steps": [{"name": "step1", "agent_id": "agent-a", "input_template": "go"}]
                        }),
                    },
                    LlmToolCall {
                        id: "tc5".to_string(),
                        name: "create_team".to_string(),
                        arguments: serde_json::json!({"id": "team-1", "name": "T1", "agent_ids": ["agent-a"]}),
                    },
                ],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "make workflow and team", "mock", "mock").collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::Done)));
        assert_eq!(store.list_workflows().unwrap().len(), 1);
        assert_eq!(store.list_teams().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_harness_list_entities_and_search() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let now = Utc::now().to_rfc3339();
        let spec = AgentSpec {
            id: AgentId("agent-x".to_string()),
            name: "Xavier".to_string(),
            description: "".to_string(),
            prompt: "".to_string(),
            tools: vec![],
            triggers: vec![Trigger::Manual],
            mcp_ids: vec![],
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let spec_json = serde_json::to_string(&spec).unwrap();
        store.insert_agent("agent-x", "Xavier", &spec_json, &spec.created_at, &spec.updated_at).unwrap();
        store.insert_team("team-1", "X-Men", "{}", &now).unwrap();

        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall {
                        id: "tc6".to_string(),
                        name: "list_entities".to_string(),
                        arguments: serde_json::json!({"entity_type": "agents"}),
                    },
                    LlmToolCall {
                        id: "tc7".to_string(),
                        name: "search_store".to_string(),
                        arguments: serde_json::json!({"query": "x", "entity_types": ["agents", "teams"]}),
                    },
                ],
            },
        ));
        let harness = Harness::new(store).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "list and search", "mock", "mock").collect().await;
        let finished: Vec<_> = events.iter().filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        }).collect();
        assert!(finished.iter().any(|r| r.contains("agent-x")));
        assert!(finished.iter().any(|r| r.contains("team-1") && r.contains("X-Men")));
    }

    #[tokio::test]
    async fn test_harness_deploy_agent_without_sender() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let _now = Utc::now().to_rfc3339();
        let spec = AgentSpec::new("A", "p");
        let spec_json = serde_json::to_string(&spec).unwrap();
        store.insert_agent("agent-1", "A", &spec_json, &spec.created_at, &spec.updated_at).unwrap();

        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc8".to_string(),
                    name: "deploy_agent".to_string(),
                    arguments: serde_json::json!({"agent_id": "agent-1", "worker_id": "w-1"}),
                }],
            },
        ));
        let harness = Harness::new(store).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "deploy", "mock", "mock").collect().await;
        let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("no deploy channel")));
        assert!(finished);
    }

    #[tokio::test]
    async fn test_harness_schedule_workflow_and_get_execution() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let wf = Workflow::new("wf", "").with_steps(vec![]);
        let spec_json = serde_json::to_string(&wf).unwrap();
        let trigger_str = serde_json::to_string(&wf.trigger).unwrap();
        store.insert_workflow(&wf.id.to_string(), &wf.name, &wf.description, &spec_json, &trigger_str, true, &wf.created_at, &wf.updated_at).unwrap();

        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc9".to_string(),
                    name: "schedule_workflow".to_string(),
                    arguments: serde_json::json!({"workflow_id": wf.id.to_string(), "trigger_type": "cron", "trigger_value": "0 9 * * *"}),
                }],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "schedule", "mock", "mock").collect().await;
        let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("scheduled")));
        assert!(finished);

        let rows = store.list_workflows().unwrap();
        assert!(rows[0].4.contains("Cron"));
    }

    #[tokio::test]
    async fn test_harness_read_write_edit_file() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall {
                        id: "tc10".to_string(),
                        name: "write_file".to_string(),
                        arguments: serde_json::json!({"path": "harness_test.txt", "content": "hello world"}),
                    },
                    LlmToolCall {
                        id: "tc11".to_string(),
                        name: "edit_file".to_string(),
                        arguments: serde_json::json!({"path": "harness_test.txt", "old_text": "world", "new_text": "Goble"}),
                    },
                    LlmToolCall {
                        id: "tc12".to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({"path": "harness_test.txt"}),
                    },
                ],
            },
        ));
        let harness = Harness::new(store).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "file ops", "mock", "mock").collect().await;
        let finished: Vec<_> = events.iter().filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        }).collect();
        assert!(finished.iter().any(|r| r.contains("harness_test.txt")));
        assert!(finished.last().unwrap().contains("hello Goble"));
        std::fs::remove_file("harness_test.txt").ok();
    }

    #[tokio::test]
    async fn test_harness_sandboxed_command_blocks_disallowed() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc13".to_string(),
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({"command": "rm", "args": ["-rf", "/"]}),
                }],
            },
        ));
        let runner = Arc::new(SandboxedCommandRunner::default_tools());
        let harness = Harness::new(store).with_llm(llm).with_runner(runner);
        let events: Vec<_> = harness.run_turn(&chat_id, "dangerous", "mock", "mock").collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallError { message, .. } if message.contains("not in the allowed list"))));
    }


    #[tokio::test]
    async fn test_harness_delete_entities() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let now = Utc::now().to_rfc3339();
        let spec = AgentSpec::new("A", "p");
        store.insert_agent("agent-1", "A", &serde_json::to_string(&spec).unwrap(), &spec.created_at, &spec.updated_at).unwrap();
        store.insert_team("team-1", "T", "{}", &now).unwrap();
        let wf = Workflow::new("wf", "").with_steps(vec![]);
        let spec_json = serde_json::to_string(&wf).unwrap();
        let trigger_str = serde_json::to_string(&wf.trigger).unwrap();
        store.insert_workflow(&wf.id.to_string(), &wf.name, &wf.description, &spec_json, &trigger_str, true, &wf.created_at, &wf.updated_at).unwrap();

        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall { id: "tc_del_a".to_string(), name: "delete_agent".to_string(), arguments: serde_json::json!({"id": "agent-1"}) },
                    LlmToolCall { id: "tc_del_t".to_string(), name: "delete_team".to_string(), arguments: serde_json::json!({"id": "team-1"}) },
                    LlmToolCall { id: "tc_del_w".to_string(), name: "delete_workflow".to_string(), arguments: serde_json::json!({"id": wf.id.to_string()}) },
                ],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        harness.run_turn(&chat_id, "delete all", "mock", "mock").collect::<Vec<_>>().await;
        assert!(store.list_agents().unwrap().is_empty());
        assert!(store.list_teams().unwrap().is_empty());
        assert!(store.list_workflows().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_harness_rename_and_delete_file() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        std::fs::write("harness_tmp.txt", "data").unwrap();
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall { id: "tc_ren".to_string(), name: "rename_file".to_string(), arguments: serde_json::json!({"from": "harness_tmp.txt", "to": "harness_renamed.txt"}) },
                ],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "rename", "mock", "mock").collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("renamed"))));
        assert!(std::fs::metadata("harness_renamed.txt").is_ok());

        let llm2 = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall { id: "tc_del".to_string(), name: "delete_file".to_string(), arguments: serde_json::json!({"path": "harness_renamed.txt"}) },
                ],
            },
        ));
        let harness2 = Harness::new(store).with_llm(llm2);
        harness2.run_turn(&chat_id, "delete", "mock", "mock").collect::<Vec<_>>().await;
        assert!(!PathBuf::from("harness_renamed.txt").exists());
    }

    #[tokio::test]
    async fn test_harness_git_status_and_run_agent() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let spec = AgentSpec::new("Greeter", "say hello");
        store.insert_agent("agent-1", "Greeter", &serde_json::to_string(&spec).unwrap(), &spec.created_at, &spec.updated_at).unwrap();

        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall { id: "tc_git".to_string(), name: "git_status".to_string(), arguments: serde_json::json!({}) },
                    LlmToolCall { id: "tc_run".to_string(), name: "run_agent".to_string(), arguments: serde_json::json!({"agent_id": "agent-1", "input": "hi"}) },
                ],
            },
        ));
        let harness = Harness::new(store).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "git and run", "mock", "mock").collect().await;
        let finished: Vec<_> = events.iter().filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        }).collect();
        assert!(finished.iter().any(|r| r.contains("mock ran") && r.contains("git")));
        assert!(finished.iter().any(|r| r.contains("ran agent") && r.contains("say hello")));
    }

    #[tokio::test]
    async fn test_harness_install_and_list_mcp() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![
                    LlmToolCall { id: "tc_mcp".to_string(), name: "install_mcp_server".to_string(), arguments: serde_json::json!({"id": "mcp-1", "name": "Files", "source": "npm", "source_value": "@modelcontextprotocol/server-files"}) },
                    LlmToolCall { id: "tc_list".to_string(), name: "list_mcp_servers".to_string(), arguments: serde_json::json!({}) },
                ],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "mcp", "mock", "mock").collect().await;
        let finished: Vec<_> = events.iter().filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        }).collect();
        assert!(finished.iter().any(|r| r.contains("mcp-1")));
        assert_eq!(store.list_mcp_servers().unwrap().len(), 1);
    }

    #[test]
    fn test_list_tools() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::new(store);
        let tools = harness.list_tools();
        assert!(tools.iter().any(|t| t.name == "create_agent"));
        assert!(tools.iter().any(|t| t.name == "search_store"));
        assert!(tools.iter().any(|t| t.name == "deploy_agent"));
        assert!(tools.iter().any(|t| t.name == "read_file"));
    }
}
