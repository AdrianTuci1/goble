use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::{Context as _, Result};
use chrono::Utc;
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::agent::{AgentId, AgentSpec, Trigger};
use crate::store::Store;
use crate::workflow::{Workflow, WorkflowStep};

/// Event emitted by the harness while processing a chat turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "payload")]
pub enum HarnessEvent {
    /// Streaming assistant text delta.
    AssistantDelta(String),
    /// Tool call started (before execution).
    ToolCallStarted {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    /// Tool call finished with a result string.
    ToolCallFinished {
        id: String,
        result: String,
    },
    /// Tool call failed.
    ToolCallError {
        id: String,
        message: String,
    },
    /// Turn finished.
    Done,
    /// Fatal harness error.
    Error(String),
}

/// Schema exposed to a frontend / LLM so it knows which tools exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolSchema {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: serde_json::Value::Object(Default::default()),
        }
    }

    pub fn with_parameters(mut self, parameters: serde_json::Value) -> Self {
        self.parameters = parameters;
        self
    }
}

/// Command runner abstraction. dyn-compatible, no `Clone`.
#[async_trait::async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &str, args: &[String]) -> Result<String>;
}

#[derive(Clone)]
struct DefaultCommandRunner;

#[async_trait::async_trait]
impl CommandRunner for DefaultCommandRunner {
    async fn run(&self, command: &str, args: &[String]) -> Result<String> {
        let output = tokio::process::Command::new(command)
            .args(args)
            .output()
            .await
            .with_context(|| format!("failed to run {command}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            anyhow::bail!(
                "command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }
    }
}

/// Command runner for tests.
#[derive(Clone)]
pub struct MockCommandRunner {
    pub output: String,
}

#[async_trait::async_trait]
impl CommandRunner for MockCommandRunner {
    async fn run(&self, _command: &str, _args: &[String]) -> Result<String> {
        Ok(self.output.clone())
    }
}

/// The harness turns user chat input into structured tool calls and streams events.
pub struct Harness {
    store: Store,
    runner: Arc<dyn CommandRunner>,
}

impl Harness {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            runner: Arc::new(DefaultCommandRunner),
        }
    }

    pub fn with_runner(store: Store, runner: Arc<dyn CommandRunner>) -> Self {
        Self { store, runner }
    }

    pub fn list_tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema::new("create_agent", "Create or update an agent.").with_parameters(
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "prompt": { "type": "string" },
                        "description": { "type": "string" },
                        "tools": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["name", "prompt"]
                }),
            ),
            ToolSchema::new("create_workflow", "Create or update a workflow.").with_parameters(
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "description": { "type": "string" },
                        "trigger": { "type": "string" },
                        "steps": { "type": "array" }
                    },
                    "required": ["name"]
                }),
            ),
            ToolSchema::new("create_team", "Create or update a team.").with_parameters(
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "name": { "type": "string" },
                        "metadata": { "type": "object" },
                        "agent_ids": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["id", "name"]
                }),
            ),
            ToolSchema::new("run_command", "Run a local shell command.").with_parameters(
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "args": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["command"]
                }),
            ),
        ]
    }

    /// Process a single user message and produce a stream of events.
    pub fn run_turn(&self,
        chat_id: &str,
        prompt: &str,
    ) -> HarnessStream {
        HarnessStream {
            state: HarnessStreamState::Start,
            store: self.store.clone(),
            runner: Arc::clone(&self.runner),
            chat_id: chat_id.to_string(),
            prompt: prompt.to_string(),
            buffer: Vec::new(),
        }
    }
}

pub struct HarnessStream {
    state: HarnessStreamState,
    store: Store,
    runner: Arc<dyn CommandRunner>,
    chat_id: String,
    prompt: String,
    buffer: Vec<HarnessEvent>,
}

enum HarnessStreamState {
    Start,
    Emitting,
    Finished,
}

impl Stream for HarnessStream {
    type Item = HarnessEvent;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.state {
            HarnessStreamState::Start => {
                self.state = HarnessStreamState::Emitting;
                let prompt = self.prompt.clone();
                match parse_command(&prompt) {
                    Some((name, args)) => {
                        let id = uuid::Uuid::new_v4().to_string();
                        self.buffer.push(HarnessEvent::ToolCallStarted {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: args.clone(),
                        });
                        let result = dispatch_tool(
                            &name,
                            &self.store,
                            &*self.runner,
                            &self.chat_id,
                            args,
                        );
                        match result {
                            Ok(value) => {
                                self.buffer.push(HarnessEvent::ToolCallFinished {
                                    id: id.clone(),
                                    result: value,
                                });
                                self.buffer.push(HarnessEvent::AssistantDelta(format!(
                                    "Done: {name}"
                                )));
                            }
                            Err(e) => {
                                self.buffer.push(HarnessEvent::ToolCallError {
                                    id: id.clone(),
                                    message: e.to_string(),
                                });
                            }
                        }
                    }
                    None => {
                        self.buffer.push(HarnessEvent::AssistantDelta(
                            "I can edit agents, workflows, teams, or run shell commands. Try /help.".to_string(),
                        ));
                    }
                }
                self.buffer.push(HarnessEvent::Done);
                self.poll_next(_cx)
            }
            HarnessStreamState::Emitting => {
                if self.buffer.is_empty() {
                    self.state = HarnessStreamState::Finished;
                    return Poll::Ready(None);
                }
                let event = self.buffer.remove(0);
                if matches!(event, HarnessEvent::Done) {
                    self.state = HarnessStreamState::Finished;
                }
                Poll::Ready(Some(event))
            }
            HarnessStreamState::Finished => Poll::Ready(None),
        }
    }
}

fn parse_command(prompt: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = prompt.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let name = parts[0].trim_start_matches('/').to_string();
    let rest = parts[1..].join(" ");

    let args = match name.as_str() {
        "create_agent" | "update_agent" => {
            let (name, prompt) = rest.split_once(' ').unwrap_or((&rest, ""));
            serde_json::json!({
                "name": name,
                "prompt": prompt,
                "description": "",
                "tools": [],
            })
        }
        "create_workflow" | "update_workflow" => {
            let (name, trigger) = rest.split_once(' ').unwrap_or((&rest, "manual"));
            serde_json::json!({
                "name": name,
                "description": "",
                "trigger": trigger,
                "steps": [],
            })
        }
        "create_team" | "update_team" => {
            let (id, name) = rest.split_once(' ').unwrap_or((&rest, ""));
            serde_json::json!({
                "id": id,
                "name": name,
                "metadata": {},
                "agent_ids": [],
            })
        }
        "run_command" => {
            let words: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            serde_json::json!({
                "command": words.first().cloned().unwrap_or_default(),
                "args": words.into_iter().skip(1).collect::<Vec<String>>(),
            })
        }
        "help" => serde_json::json!({}),
        _ => return None,
    };

    Some((name, args))
}

fn dispatch_tool(
    name: &str,
    store: &Store,
    runner: &dyn CommandRunner,
    _chat_id: &str,
    args: serde_json::Value,
) -> Result<String> {
    match name {
        "create_agent" | "update_agent" => edit_agent(store, &args),
        "create_workflow" | "update_workflow" => edit_workflow(store, &args),
        "create_team" | "update_team" => edit_team(store, &args),
        "run_command" => {
            let command = args["command"].as_str().unwrap_or_default();
            let args: Vec<String> = args["args"]
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
        "help" => Ok("Commands: /create_agent name prompt, /create_workflow name trigger, /create_team id name, /run_command cmd args".to_string()),
        _ => anyhow::bail!("unknown tool: {name}"),
    }
}

fn edit_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let name = args["name"].as_str().unwrap_or("unnamed");
    let prompt = args["prompt"].as_str().unwrap_or("");
    let description = args["description"].as_str().unwrap_or("");
    let tools: Vec<String> = args["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let spec = AgentSpec::new(name, prompt).with_description(description).with_tools(tools);
    let id = spec.id.clone();
    let now = Utc::now().to_rfc3339();
    store.insert_agent(
        &id.to_string(),
        name,
        &serde_json::to_string(&spec)?,
        &now,
        &now,
    )?;
    Ok(format!("agent {} created/updated", id))
}

fn edit_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let name = args["name"].as_str().unwrap_or("unnamed");
    let description = args["description"].as_str().unwrap_or("");
    let trigger_str = args["trigger"].as_str().unwrap_or("manual");
    let trigger = match trigger_str {
        "manual" => Trigger::Manual,
        other => Trigger::Cron {
            expression: other.to_string(),
        },
    };
    let steps: Vec<WorkflowStep> = args["steps"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let name = v["name"].as_str()?;
                    let agent_id = v["agent_id"].as_str()?;
                    Some(WorkflowStep {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: name.to_string(),
                        agent_id: AgentId(agent_id.to_string()),
                        input_template: v["input_template"].as_str().unwrap_or("").to_string(),
                        depends_on: vec![],
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let wf = Workflow::new(name, description)
        .with_trigger(trigger)
        .with_steps(steps);
    let id = wf.id.clone();
    let now = Utc::now().to_rfc3339();
    store.insert_workflow(
        &id.to_string(),
        name,
        description,
        &serde_json::to_string(&wf)?,
        &serde_json::to_string(&wf.trigger)?,
        wf.enabled,
        &now,
        &now,
    )?;
    Ok(format!("workflow {} created/updated", id))
}

fn edit_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().unwrap_or_default();
    let name = args["name"].as_str().unwrap_or("unnamed");
    let metadata = args["metadata"].clone();
    let agent_ids: Vec<String> = args["agent_ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    store.insert_team(id, name, &metadata.to_string(), &now)?;
    for agent_id in &agent_ids {
        store.insert_team_member(id, agent_id)?;
    }
    Ok(format!("team {id} created/updated with {} members", agent_ids.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_harness_create_agent() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::new(store);
        let stream = harness.run_turn("chat1", "/create_agent greeter say hello");
        let events: Vec<_> = stream.collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallStarted { name, .. } if name == "create_agent")));
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("agent"))));
        assert!(events.iter().any(|e| e == &HarnessEvent::Done));
    }

    #[tokio::test]
    async fn test_harness_run_command_mock() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::with_runner(store, Arc::new(MockCommandRunner { output: "mocked output".to_string() }));
        let stream = harness.run_turn("chat1", "/run_command echo hello");
        let events: Vec<_> = stream.collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result == "mocked output")));
    }

    #[tokio::test]
    async fn test_harness_unknown_command() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::new(store);
        let stream = harness.run_turn("chat1", "hello world");
        let events: Vec<_> = stream.collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::AssistantDelta(delta) if delta.contains("/help"))));
    }

    #[tokio::test]
    async fn test_harness_create_workflow_and_team() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::new(store);
        let stream = harness.run_turn("chat1", "/create_workflow daily manual");
        let events: Vec<_> = stream.collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("workflow"))));

        let stream = harness.run_turn("chat1", "/create_team platform PlatformTeam");
        let events: Vec<_> = stream.collect().await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("team"))));
    }

    #[test]
    fn test_parse_command() {
        assert!(parse_command("hello").is_none());
        let (name, args) = parse_command("/create_agent greeter say hello").unwrap();
        assert_eq!(name, "create_agent");
        assert_eq!(args["name"], "greeter");
        assert_eq!(args["prompt"], "say hello");
    }

    #[test]
    fn test_list_tools() {
        let store = Store::open_in_memory().unwrap();
        let harness = Harness::new(store);
        let tools = harness.list_tools();
        assert_eq!(tools.len(), 4);
        assert!(tools.iter().any(|t| t.name == "create_agent"));
    }
}
