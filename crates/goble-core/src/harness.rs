use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use chrono::Utc;
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::agent::{AgentId, AgentSpec, Trigger};
use crate::llm::{CompletionRequest, CompletionResponse, LlmProvider, LlmToolCall, Role, ToolDefinition};
use crate::store::Store;
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
    ) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
        let store = self.store.clone();
        let runner = Arc::clone(&self.runner);
        let llm = Arc::clone(&self.llm);
        let chat_id = chat_id.to_string();
        let prompt = prompt.to_string();

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
                    .map(|(_, role, content, _, _)| crate::llm::Message {
                        role: match role.as_str() {
                            "system" => Role::System,
                            "assistant" => Role::Assistant,
                            "tool" => Role::Tool,
                            _ => Role::User,
                        },
                        content,
                        tool_calls: None,
                    })
                    .collect::<Vec<_>>(),
                Err(e) => {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
            };

            let tools = harness_tool_definitions();
            let request = CompletionRequest::new(llm.name(), "gpt-4o-mini")
                .with_system(HARNESS_SYSTEM_PROMPT)
                .with_tools(tools)
                .with_messages(history);

            let response = match llm.complete(request).await {
                Ok(r) => r,
                Err(e) => {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
            };

            // Stream assistant text delta as a single chunk for now.
            let assistant_content = response.content.clone();
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
                yield HarnessEvent::AssistantDelta(assistant_content);
            }

            // Persist tool calls before executing.
            let tool_calls = response.tool_calls.clone();
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

            for call in tool_calls {
                yield HarnessEvent::ToolCallStarted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                };

                let result = execute_tool_call(&store, &*runner, &call).await;
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
                        yield HarnessEvent::ToolCallFinished { id: call.id, result: value };
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
                        yield HarnessEvent::ToolCallError { id: call.id, message: e.to_string() };
                    }
                }
            }

            yield HarnessEvent::Done;
        };

        Box::pin(stream)
    }
}

const HARNESS_SYSTEM_PROMPT: &str = r#"You are an assistant that controls a local Goble agent environment.
You can create and update agents, workflows and teams, and run shell commands, by calling tools.
Only call a tool when the user explicitly asks for an action you can perform with a tool.
If no tool is needed, reply conversationally."#;

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
            description: "Run a shell command. Use with care.".to_string(),
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
            description: "List existing agents, workflows or teams.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "entity_type": { "type": "string", "enum": ["agents", "workflows", "teams"] }
                },
                "required": ["entity_type"]
            }),
        },
    ]
}

async fn execute_tool_call(
    store: &Store,
    runner: &dyn CommandRunner,
    call: &LlmToolCall,
) -> Result<String> {
    match call.name.as_str() {
        "create_agent" => {
            let id = call.arguments["id"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let name = call.arguments["name"].as_str().unwrap_or(&id).to_string();
            let description = call.arguments["description"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let prompt = call.arguments["prompt"]
                .as_str()
                .context("prompt is required")?
                .to_string();
            let tools = call.arguments["tools"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let mcp_ids = call.arguments["mcp_ids"]
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
        "update_agent" => update_agent(store, &call.arguments),
        "create_workflow" => create_workflow(store, &call.arguments),
        "update_workflow" => update_workflow(store, &call.arguments),
        "create_team" => create_team(store, &call.arguments),
        "update_team" => update_team(store, &call.arguments),
        "run_command" => {
            let command = call.arguments["command"].as_str().unwrap_or_default();
            let args: Vec<String> = call.arguments["args"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            // Avoid nested block_on: run via a fresh thread when inside a tokio runtime.
            if let Ok(_handle) = Handle::try_current() {
                std::thread::scope(|s| {
                    s.spawn(|| {
                        let runtime = tokio::runtime::Runtime::new()?;
                        runtime.block_on(runner.run(command, &args))
                    }).join().unwrap()
                })
            } else {
                let runtime = tokio::runtime::Runtime::new()?;
                runtime.block_on(runner.run(command, &args))
            }
        }
        "list_entities" => {
            let entity_type = call.arguments["entity_type"].as_str().unwrap_or("agents");
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
                _ => Ok(format!("unknown entity type {entity_type}")),
            }
        }
        _ => anyhow::bail!("unknown tool {}", call.name),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockProvider;
    use futures::StreamExt;

    fn chat(store: &Store) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        store.insert_chat(&id, "test", &now, &now).unwrap();
        id
    }

    #[tokio::test]
    async fn test_harness_create_agent() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc1".to_string(),
                    name: "create_agent".to_string(),
                    arguments: serde_json::json!({
                        "id": "agent-1",
                        "name": "Greeter",
                        "prompt": "Say hello"
                    }),
                }],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "make a greeter").collect().await;
        let started = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallStarted { name, .. } if name == "create_agent"));
        assert!(started);
        let agents = store.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].0, "agent-1");
    }

    #[tokio::test]
    async fn test_harness_run_command_mock() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc2".to_string(),
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({"command": "echo", "args": ["hi"]}),
                }],
            },
        ));
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness.run_turn(&chat_id, "run echo hi").collect().await;
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
        let events: Vec<_> = harness.run_turn(&chat_id, "do it").collect().await;
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
        let events: Vec<_> = harness.run_turn(&chat_id, "make workflow and team").collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::Done)));
        assert_eq!(store.list_workflows().unwrap().len(), 1);
        assert_eq!(store.list_teams().unwrap().len(), 1);
    }

    #[test]
    fn test_list_tools() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::new(store);
        let tools = harness.list_tools();
        assert!(tools.iter().any(|t| t.name == "create_agent"));
    }
}
