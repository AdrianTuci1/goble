use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::harness::{
    arc_to_sender_ref, command_candidates, execute_tool_call, harness_tool_definitions,
    run_approved_command, ChatToolCall, CommandDecision, HarnessEvent, ThinkingMode,
    ToolCallStatus, WebSearchConfig, COMMAND_TOOL, HARNESS_SYSTEM_PROMPT,
};
use crate::llm::{
    CompletionRequest, CompletionStreamEvent, LlmProvider, LlmToolCall, Message, Role,
    ToolDefinition,
};
use crate::mcp_manager::McpManager;
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::worker::WorkerId;

const MAX_REASONING_STEPS: usize = 6;
const MAX_EXECUTION_STEPS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step: usize,
    pub mode: ThinkingMode,
    pub content: String,
    pub decision: ReasoningDecision,
    pub tool_calls: Vec<LlmToolCall>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningDecision {
    Continue,
    Execute,
    AskUser,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionState {
    pub id: String,
    pub chat_id: String,
    pub goal: String,
    pub status: String,
    pub plan: Option<String>,
    pub workflow_id: Option<String>,
    pub reasoning_steps: Vec<ReasoningStep>,
    pub pending_ask: Option<PendingAsk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAsk {
    pub id: String,
    pub question: String,
    pub quick_replies: Vec<String>,
}

pub fn build_reasoning_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "set_thinking_mode".to_string(),
            description: "Switch the thinking mode for the next reasoning step.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["direct", "contemplating", "ruminating", "baking", "reflecting", "verifying", "debugging", "synthesizing", "planning"] }
                },
                "required": ["mode"]
            }),
        },
        ToolDefinition {
            name: "continue_thinking".to_string(),
            description: "Continue reasoning for another step. Optionally provide a focus prompt."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "execute".to_string(),
            description: "Stop reasoning and proceed to execute the plan or reply.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ask_user".to_string(),
            description: "Ask the user for clarification before continuing.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "quick_replies": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["question"]
            }),
        },
        ToolDefinition {
            name: "create_mission".to_string(),
            description: "Create or update a mission tracking a complex goal.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "goal": { "type": "string" },
                    "status": { "type": "string", "enum": ["clarifying", "planning", "deploying", "running", "done", "error"] },
                    "plan": { "type": "string" },
                    "workflow_id": { "type": "string" }
                },
                "required": ["goal"]
            }),
        },
        ToolDefinition {
            name: "update_mission".to_string(),
            description: "Update mission status or plan.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "status": { "type": "string", "enum": ["clarifying", "planning", "deploying", "running", "done", "error"] },
                    "plan": { "type": "string" },
                    "workflow_id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
    ]
}

fn build_reasoning_prompt(mode: ThinkingMode, goal: &str, steps: &[ReasoningStep]) -> String {
    let mut prompt = format!(
        "You are in thinking mode: {}.\n{}\n\n",
        mode.as_str(),
        mode.prompt()
    );
    prompt.push_str("You are orchestrating a complex task. You may call reasoning tools: set_thinking_mode, continue_thinking, execute, ask_user, create_mission, update_mission.\n");
    prompt.push_str("Rules:\n");
    prompt.push_str("- If you need more information, call ask_user.\n");
    prompt.push_str(
        "- If you need to keep reasoning, call continue_thinking or set_thinking_mode.\n",
    );
    prompt.push_str("- When you are ready to act (call tools, create agents/workflows, deploy), call execute.\n");
    prompt.push_str("- Track the overall goal using create_mission / update_mission.\n\n");
    prompt.push_str(&format!("Current goal: {goal}\n\n"));
    if !steps.is_empty() {
        prompt.push_str("Previous reasoning steps:\n");
        for step in steps.iter().rev().take(4) {
            prompt.push_str(&format!(
                "- [{}] {} -> {}\n",
                step.step,
                step.mode.as_str(),
                serde_json::to_string(&step.decision).unwrap_or_default()
            ));
        }
        prompt.push('\n');
    }
    prompt
}

fn build_execution_prompt(goal: &str, reasoning_steps: &[ReasoningStep]) -> String {
    let mut prompt = format!("You are now executing. The goal is: {goal}\n\nReasoning summary:\n");
    for step in reasoning_steps.iter().rev().take(6) {
        let content = step.content.chars().take(200).collect::<String>();
        prompt.push_str(&format!(
            "- [{}] {}: {}\n",
            step.step,
            step.mode.as_str(),
            content
        ));
    }
    prompt.push_str("\nUse available tools to create agents, workflows, discover MCPs, install/connect MCPs, deploy to workers, and schedule workflows.\n");
    prompt.push_str("Only call a tool when you have enough information. If still missing info, ask the user instead.\n");
    prompt
}

fn load_or_create_mission(store: &Store, chat_id: &str, goal: &str) -> Result<MissionState> {
    let missions = store.list_missions()?;
    if let Some(mission) = missions
        .into_iter()
        .find(|m| m.1 == chat_id && m.3 != "done")
    {
        let reasoning = store
            .list_reasoning_steps(&mission.0)?
            .into_iter()
            .map(|row| ReasoningStep {
                step: row.1 as usize,
                mode: row.2.parse().unwrap_or(ThinkingMode::Direct),
                content: row.3,
                decision: row
                    .4
                    .as_deref()
                    .and_then(|d| serde_json::from_str(d).ok())
                    .unwrap_or(ReasoningDecision::Continue),
                tool_calls: row
                    .5
                    .as_deref()
                    .map(|t| serde_json::from_str(t).unwrap_or_default())
                    .unwrap_or_default(),
            })
            .collect();
        let pending = store.get_pending_ask(chat_id)?;
        return Ok(MissionState {
            id: mission.0,
            chat_id: mission.1,
            goal: mission.2,
            status: mission.3,
            plan: mission.4,
            workflow_id: mission.5,
            reasoning_steps: reasoning,
            pending_ask: pending.map(|p| PendingAsk {
                id: p.0,
                question: p.3,
                quick_replies: p.4.split('\n').map(|s| s.to_string()).collect(),
            }),
        });
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    store.insert_mission(&id, chat_id, goal, "clarifying", None, None, &now, &now)?;
    Ok(MissionState {
        id,
        chat_id: chat_id.to_string(),
        goal: goal.to_string(),
        status: "clarifying".to_string(),
        plan: None,
        workflow_id: None,
        reasoning_steps: Vec::new(),
        pending_ask: None,
    })
}

fn persist_mission(store: &Store, mission: &MissionState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    store.insert_mission(
        &mission.id,
        &mission.chat_id,
        &mission.goal,
        &mission.status,
        mission.plan.as_deref(),
        mission.workflow_id.as_deref(),
        &now,
        &now,
    )?;
    Ok(())
}

fn persist_reasoning_step(store: &Store, mission_id: &str, step: &ReasoningStep) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    store.insert_reasoning_step(
        &uuid::Uuid::new_v4().to_string(),
        mission_id,
        step.step as i32,
        step.mode.as_str(),
        &step.content,
        Some(&serde_json::to_string(&step.decision)?),
        Some(&serde_json::to_string(&step.tool_calls)?),
        &now,
    )?;
    Ok(())
}

/// Rewrite the assistant row's `tool_calls` column with the current records, so
/// a call's status/result is readable from the store while the turn still runs.
fn persist_tool_call_records(
    store: &Store,
    message_id: &str,
    records: &[ChatToolCall],
) -> Result<()> {
    let json = serde_json::to_string(records)?;
    store.set_chat_message_tool_calls(message_id, &json)
}

fn parse_reasoning_tool_calls(
    tool_calls: &[LlmToolCall],
    mode: &mut ThinkingMode,
) -> (ReasoningDecision, Vec<String>) {
    let mut decision = ReasoningDecision::Continue;
    let mut focus_notes = Vec::new();

    for call in tool_calls {
        match call.name.as_str() {
            "set_thinking_mode" => {
                if let Some(m) = call.arguments["mode"].as_str() {
                    if let Ok(new_mode) = m.parse() {
                        *mode = new_mode;
                    }
                }
            }
            "continue_thinking" => {
                decision = ReasoningDecision::Continue;
                if let Some(focus) = call.arguments["focus"].as_str() {
                    focus_notes.push(focus.to_string());
                }
            }
            "execute" => {
                decision = ReasoningDecision::Execute;
            }
            "ask_user" => {
                decision = ReasoningDecision::AskUser;
            }
            "create_mission" | "update_mission" => {
                decision = ReasoningDecision::Continue;
            }
            _ => {}
        }
    }

    (decision, focus_notes)
}

pub fn run_mission_turn(
    store: Store,
    runner: Arc<dyn crate::harness::CommandRunner>,
    llm: Arc<dyn LlmProvider>,
    deploy_sender: Option<Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>>,
    mcp_manager: McpManager,
    cancel: Arc<AtomicBool>,
    workspace_dir: std::path::PathBuf,
    docs_dir: std::path::PathBuf,
    chat_id: String,
    prompt: String,
    provider: String,
    model: String,
    reasoning_enabled: bool,
    auto_approve: bool,
    web_search: WebSearchConfig,
) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
    Box::pin(async_stream::stream! {
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

        let mut mission = match load_or_create_mission(&store, &chat_id, &prompt) {
            Ok(m) => m,
            Err(e) => {
                yield HarnessEvent::Error(e.to_string());
                return;
            }
        };

        yield HarnessEvent::MissionUpdated {
            mission_id: mission.id.clone(),
            status: mission.status.clone(),
        };

        let mut mode = ThinkingMode::Direct;
        if !mission.reasoning_steps.is_empty() {
            mode = mission.reasoning_steps.last().unwrap().mode;
        }

        let mut tools = harness_tool_definitions();
        if let Ok(mcp_tools) = mcp_manager.refresh_from_store(&store) {
            tools.extend(mcp_tools);
        }
        tools.push(McpManager::generic_mcp_call_tool());

        let reasoning_tools = build_reasoning_tools();
        let enable_reasoning = reasoning_enabled && (is_orchestration_goal(&prompt) || !mission.reasoning_steps.is_empty() || mission.pending_ask.is_some());

        let mut reasoning_step = 0usize;
        let mut pending_decision = ReasoningDecision::Execute;

        if enable_reasoning {
            while reasoning_step < MAX_REASONING_STEPS {
                if cancel.load(Ordering::Relaxed) {
                    yield HarnessEvent::Error("cancelled".to_string());
                    return;
                }

                let history = build_history(&store, &chat_id, &mission, &reasoning_tools, &tools, mode).await;
                let request = CompletionRequest::new(provider.clone(), model.clone())
                    .with_system(HARNESS_SYSTEM_PROMPT)
                    .with_system(build_reasoning_prompt(mode, &mission.goal, &mission.reasoning_steps))
                    .with_messages(history)
                    .with_tools(reasoning_tools.clone());

                yield HarnessEvent::ReasoningStarted {
                    step: reasoning_step,
                    mode: mode.as_str().to_string(),
                };

                let mut stream = match llm.complete_stream(request).await {
                    Ok(s) => s,
                    Err(e) => {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                };

                let mut content = String::new();
                let mut tool_calls = Vec::new();
                while let Some(event) = stream.next().await {
                    if cancel.load(Ordering::Relaxed) {
                        yield HarnessEvent::Error("cancelled".to_string());
                        return;
                    }
                    match event {
                        CompletionStreamEvent::AssistantDelta(delta) => {
                            content.push_str(&delta);
                            yield HarnessEvent::ReasoningDelta(delta);
                        }
                        CompletionStreamEvent::ToolCalls(calls) => {
                            tool_calls = calls;
                        }
                        CompletionStreamEvent::Done => break,
                        CompletionStreamEvent::Error(message) => {
                            yield HarnessEvent::Error(message);
                            return;
                        }
                    }
                }

                let (decision, _focus) = parse_reasoning_tool_calls(&tool_calls, &mut mode);
                let step = ReasoningStep {
                    step: reasoning_step,
                    mode,
                    content: content.clone(),
                    decision: decision.clone(),
                    tool_calls: tool_calls.clone(),
                };
                if let Err(e) = persist_reasoning_step(&store, &mission.id, &step) {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
                mission.reasoning_steps.push(step);

                yield HarnessEvent::ReasoningDone {
                    step: reasoning_step,
                    mode: mode.as_str().to_string(),
                    content: content.clone(),
                    decision: serde_json::to_string(&decision).unwrap_or_default(),
                };

                if decision == ReasoningDecision::Execute || decision == ReasoningDecision::Done {
                    pending_decision = decision;
                    break;
                }

                if decision == ReasoningDecision::AskUser {
                    if let Some(ask) = extract_ask_user(&tool_calls) {
                        if auto_approve {
                            // Auto-approve: skip suspending on the question and
                            // record a synthetic answer so the next reasoning /
                            // execution step sees the user already responded.
                            let now = Utc::now().to_rfc3339();
                            if let Err(e) = store.insert_chat_message(
                                &uuid::Uuid::new_v4().to_string(),
                                &chat_id,
                                "user",
                                &format!("(auto-approved; no answer to: {})", ask.question),
                                None,
                                &now,
                            ) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
                        } else {
                            let now = Utc::now().to_rfc3339();
                            if let Err(e) = store.insert_pending_ask(
                                &ask.id,
                                &chat_id,
                                Some(&mission.id),
                                &ask.question,
                                &ask.quick_replies.join("\n"),
                                "pending",
                                &now,
                                &now,
                            ) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
                            mission.pending_ask = Some(ask.clone());
                            yield HarnessEvent::AskUser {
                                question: ask.question,
                                quick_replies: ask.quick_replies,
                            };
                            return;
                        }
                    }
                }

                reasoning_step += 1;
                if reasoning_step >= MAX_REASONING_STEPS {
                    pending_decision = ReasoningDecision::Execute;
                    break;
                }
            }

            if pending_decision == ReasoningDecision::Done {
                yield HarnessEvent::Done;
                return;
            }
        } // end if enable_reasoning

        // Execution phase
        let mut execution_iteration = 0;
        let mut prev_tool_calls: Vec<LlmToolCall> = Vec::new();

        loop {
            if cancel.load(Ordering::Relaxed) {
                yield HarnessEvent::Error("cancelled".to_string());
                return;
            }
            if execution_iteration >= MAX_EXECUTION_STEPS {
                yield HarnessEvent::Error("too many execution iterations".to_string());
                return;
            }
            execution_iteration += 1;

            let mut history = build_history(&store, &chat_id, &mission, &reasoning_tools, &tools, mode).await;
            history.push(Message {
                role: Role::System,
                content: build_execution_prompt(&mission.goal, &mission.reasoning_steps),
                tool_calls: None,
                tool_call_id: None,
            });

            let request = CompletionRequest::new(provider.clone(), model.clone())
                .with_system(HARNESS_SYSTEM_PROMPT)
                .with_messages(history)
                .with_tools(tools.clone());

            let mut stream = match llm.complete_stream(request).await {
                Ok(s) => s,
                Err(e) => {
                    yield HarnessEvent::Error(e.to_string());
                    return;
                }
            };

            let mut assistant_content = String::new();
            let mut tool_calls = Vec::new();
            // Stream the assistant reply into a single chat message row as deltas
            // arrive, so the renderer can show it progressively instead of only at
            // the end of the turn. The message id is kept to attach tool-call
            // metadata once the stream finishes.
            let mut assistant_msg_id: Option<String> = None;
            while let Some(event) = stream.next().await {
                if cancel.load(Ordering::Relaxed) {
                    yield HarnessEvent::Error("cancelled".to_string());
                    break;
                }
                match event {
                    CompletionStreamEvent::AssistantDelta(delta) => {
                        assistant_content.push_str(&delta);
                        match &assistant_msg_id {
                            Some(id) => {
                                if let Err(e) = store.append_chat_message_content(id, &delta) {
                                    yield HarnessEvent::Error(e.to_string());
                                    return;
                                }
                            }
                            None => {
                                let id = uuid::Uuid::new_v4().to_string();
                                if let Err(e) = store.insert_chat_message(
                                    &id,
                                    &chat_id,
                                    "assistant",
                                    &delta,
                                    None,
                                    &Utc::now().to_rfc3339(),
                                ) {
                                    yield HarnessEvent::Error(e.to_string());
                                    return;
                                }
                                assistant_msg_id = Some(id);
                            }
                        }
                        yield HarnessEvent::AssistantDelta(delta);
                    }
                    CompletionStreamEvent::ToolCalls(calls) => {
                        tool_calls = calls;
                    }
                    CompletionStreamEvent::Done => break,
                    CompletionStreamEvent::Error(message) => {
                        yield HarnessEvent::Error(message);
                        return;
                    }
                }
            }

            if tool_calls.is_empty() {
                break;
            }
            if assistant_content.is_empty() {
                if tool_calls == prev_tool_calls {
                    break;
                }
                prev_tool_calls = tool_calls.clone();
            }

            // Attach tool-call metadata to the streamed message, or create the
            // tool-call-only assistant message, so the next iteration's history
            // carries the calls. The records carry a lifecycle status that is
            // rewritten as each call runs, so a call is readable from the store
            // while the turn is still in flight instead of only once it ends.
            let mut call_records: Vec<ChatToolCall> =
                tool_calls.iter().map(ChatToolCall::planned).collect();
            let tool_calls_json = serde_json::to_string(&call_records).unwrap_or_default();
            match &assistant_msg_id {
                Some(id) => {
                    if let Err(e) = store.set_chat_message_tool_calls(id, &tool_calls_json) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                }
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    if let Err(e) = store.insert_chat_message(
                        &id,
                        &chat_id,
                        "assistant",
                        "",
                        Some(&tool_calls_json),
                        &Utc::now().to_rfc3339(),
                    ) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                    assistant_msg_id = Some(id);
                }
            }

            for (index, call) in tool_calls.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    yield HarnessEvent::Error("cancelled".to_string());
                    break;
                }
                call_records[index].status = ToolCallStatus::Running;
                if let Some(id) = &assistant_msg_id {
                    if let Err(e) = persist_tool_call_records(&store, id, &call_records) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                }
                yield HarnessEvent::ToolCallStarted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                };

                // A command tool suspends here, before it runs, until the user
                // approves, edits or rejects it. `auto_approve` is the single
                // switch that skips the gate and runs the command immediately.
                if !auto_approve && call.name == COMMAND_TOOL {
                    let candidates = command_candidates(call);
                    let cwd = workspace_dir.display().to_string();
                    let now = Utc::now().to_rfc3339();
                    let candidates_json = match serde_json::to_string(&candidates) {
                        Ok(json) => json,
                        Err(e) => {
                            yield HarnessEvent::Error(e.to_string());
                            return;
                        }
                    };
                    let message_id = assistant_msg_id.clone().unwrap_or_default();
                    if let Err(e) = store.insert_pending_command(
                        &call.id,
                        &chat_id,
                        &message_id,
                        &candidates_json,
                        &cwd,
                        "pending",
                        &now,
                        &now,
                    ) {
                        yield HarnessEvent::Error(e.to_string());
                        return;
                    }
                    yield HarnessEvent::CommandProposed {
                        id: call.id.clone(),
                        candidates,
                        cwd,
                    };
                    return;
                }

                let sender_ref = deploy_sender.as_ref().map(|f| arc_to_sender_ref(f));
                let result = execute_tool_call(&store, &*runner, sender_ref, &mcp_manager, &workspace_dir, &docs_dir, call, &web_search
                ).await;
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
                        call_records[index].status = ToolCallStatus::Finished;
                        call_records[index].result = Some(value.clone());
                        if let Some(id) = &assistant_msg_id {
                            if let Err(e) = persist_tool_call_records(&store, id, &call_records) {
                                yield HarnessEvent::Error(e.to_string());
                                return;
                            }
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
                        call_records[index].status = ToolCallStatus::Error;
                        call_records[index].result = Some(e.to_string());
                        if let Some(id) = &assistant_msg_id {
                            if let Err(e2) = persist_tool_call_records(&store, id, &call_records) {
                                yield HarnessEvent::Error(e2.to_string());
                                return;
                            }
                        }
                        yield HarnessEvent::ToolCallError { id: call.id.clone(), message: e.to_string() };
                    }
                }
            }
        }

        mission.status = "done".to_string();
        if let Err(e) = persist_mission(&store, &mission) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }
        yield HarnessEvent::MissionUpdated {
            mission_id: mission.id.clone(),
            status: mission.status.clone(),
        };
        yield HarnessEvent::Done;
    })
}

pub fn resume_mission_turn(
    store: Store,
    runner: Arc<dyn crate::harness::CommandRunner>,
    llm: Arc<dyn LlmProvider>,
    deploy_sender: Option<Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>>,
    mcp_manager: McpManager,
    cancel: Arc<AtomicBool>,
    workspace_dir: std::path::PathBuf,
    docs_dir: std::path::PathBuf,
    chat_id: String,
    response: String,
    credential: Option<(String, String)>,
    provider: String,
    model: String,
    auto_approve: bool,
    web_search: WebSearchConfig,
) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
    Box::pin(async_stream::stream! {
        let now = Utc::now().to_rfc3339();
        let ask = match store.get_pending_ask(&chat_id) {
            Ok(Some(a)) => a,
            _ => {
                yield HarnessEvent::Error("no pending ask for chat".to_string());
                return;
            }
        };

        if let Err(e) = store.resolve_pending_ask(&ask.0, "answered", &now) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }

        let mut answer = format!("Answer to question '{}': {}", ask.3, response);
        // A credential entered in the ask card is stored by name and referenced
        // in the transcript by that name only, so the raw secret never reaches
        // the model or the conversation history.
        if let Some((name, value)) = credential {
            let name = if name.trim().is_empty() {
                format!("cred_{}", uuid::Uuid::new_v4().simple())
            } else {
                name
            };
            if let Err(e) = store.set_credential(&name, &value) {
                yield HarnessEvent::Error(e.to_string());
                return;
            }
            answer.push_str(&format!(
                "\nCredential stored as {name}. Reference it in run_command as {{{{credential:{name}}}}}."
            ));
        }
        if let Err(e) = store.insert_chat_message(
            &uuid::Uuid::new_v4().to_string(),
            &chat_id,
            "user",
            &answer,
            None,
            &now,
        ) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }

        let mut inner = run_mission_turn(
            store,
            runner,
            llm,
            deploy_sender,
            mcp_manager,
            cancel,
            workspace_dir,
            docs_dir,
            chat_id,
            response,
            provider,
            model,
            true,
            auto_approve,
            web_search,
        );
        while let Some(event) = inner.next().await {
            yield event;
        }
    })
}

/// Resume a turn that suspended on a proposed command, executing the user's
/// decision and recording the outcome (A6).
///
/// `Approve` runs the chosen text verbatim, `Edit` runs the edited text, and
/// `Reject` fails the tool call with a `ToolCallError` instead of running
/// anything. The result (or the rejection) is persisted into the call's record
/// and as a `role="tool"` row, exactly as the immediate path writes it.
pub fn resume_command_turn(
    store: Store,
    runner: Arc<dyn crate::harness::CommandRunner>,
    chat_id: String,
    decision: CommandDecision,
) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
    Box::pin(async_stream::stream! {
        let now = Utc::now().to_rfc3339();
        let pending = match store.get_pending_command(&chat_id) {
            Ok(Some(pending)) => pending,
            _ => {
                yield HarnessEvent::Error("no pending command for chat".to_string());
                return;
            }
        };
        let (call_id, message_id, _candidates, _cwd) = pending;
        if let Err(e) = store.resolve_pending_command(&call_id, "resolved", &now) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }

        let outcome = match decision {
            CommandDecision::Reject(reason) => {
                let message = if reason.trim().is_empty() {
                    "rejected by user".to_string()
                } else {
                    format!("rejected by user: {reason}")
                };
                Err(anyhow::anyhow!(message))
            }
            CommandDecision::Approve(text) | CommandDecision::Edit(text) => {
                run_approved_command(&store, &*runner, &text).await
            }
        };

        let (status, body) = match &outcome {
            Ok(value) => (ToolCallStatus::Finished, value.clone()),
            Err(e) => (ToolCallStatus::Error, e.to_string()),
        };
        if let Err(e) = record_command_outcome(&store, &chat_id, &message_id, &call_id, status, &body) {
            yield HarnessEvent::Error(e.to_string());
            return;
        }

        match outcome {
            Ok(value) => yield HarnessEvent::ToolCallFinished { id: call_id, result: value },
            Err(e) => yield HarnessEvent::ToolCallError { id: call_id, message: e.to_string() },
        }
        yield HarnessEvent::Done;
    })
}

/// Write a resumed command's outcome to the store: the tool-result row the
/// immediate path writes, plus the call's `status`/`result` on the assistant row
/// it was planned on.
fn record_command_outcome(
    store: &Store,
    chat_id: &str,
    message_id: &str,
    call_id: &str,
    status: ToolCallStatus,
    body: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let text = if status == ToolCallStatus::Error {
        format!("{call_id}\nERROR: {body}")
    } else {
        format!("{call_id}\n{body}")
    };
    store.insert_chat_message(
        &uuid::Uuid::new_v4().to_string(),
        chat_id,
        "tool",
        &text,
        None,
        &now,
    )?;

    if message_id.is_empty() {
        return Ok(());
    }
    let Some((_, _, _, Some(tool_calls), _)) = store
        .list_chat_messages(chat_id)?
        .into_iter()
        .find(|(id, _, _, _, _)| id == message_id)
    else {
        return Ok(());
    };
    let mut records: Vec<ChatToolCall> = serde_json::from_str(&tool_calls).unwrap_or_default();
    if let Some(record) = records.iter_mut().find(|r| r.id == call_id) {
        record.status = status;
        record.result = Some(body.to_string());
        persist_tool_call_records(store, message_id, &records)?;
    }
    Ok(())
}

async fn build_history(
    store: &Store,
    chat_id: &str,
    _mission: &MissionState,
    _reasoning_tools: &[ToolDefinition],
    _execution_tools: &[ToolDefinition],
    _mode: ThinkingMode,
) -> Vec<Message> {
    match store.list_chat_messages(chat_id) {
        Ok(rows) => rows
            .into_iter()
            .map(|(_, role, content, tool_calls, _)| {
                let (content, tool_call_id) = if role.as_str() == "tool" {
                    if let Some((id, rest)) = content.split_once('\n') {
                        (rest.to_string(), Some(id.to_string()))
                    } else {
                        (content, None)
                    }
                } else {
                    (content, None)
                };
                Message {
                    role: match role.as_str() {
                        "system" => Role::System,
                        "assistant" => Role::Assistant,
                        "tool_calls" => Role::Assistant,
                        "tool" => Role::Tool,
                        _ => Role::User,
                    },
                    content,
                    tool_calls: tool_calls.and_then(|t| serde_json::from_str(&t).ok()),
                    tool_call_id,
                }
            })
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    }
}

fn extract_ask_user(tool_calls: &[LlmToolCall]) -> Option<PendingAsk> {
    for call in tool_calls {
        if call.name == "ask_user" {
            let question = call.arguments["question"].as_str()?.to_string();
            let quick_replies: Vec<String> = call.arguments["quick_replies"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            return Some(PendingAsk {
                id: uuid::Uuid::new_v4().to_string(),
                question,
                quick_replies,
            });
        }
    }
    None
}

fn is_orchestration_goal(prompt: &str) -> bool {
    let prompt_lower = prompt.to_lowercase();
    let triggers = [
        "agent",
        "workflow",
        "mcp",
        "deploy",
        "schedule",
        "cron",
        "worker",
        "orchestr",
        "automate",
        "build a",
        "create a",
        "mission",
        "plan",
        "multiple steps",
        "complex",
    ];
    triggers.iter().any(|t| prompt_lower.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Harness;
    use crate::llm::{CompletionResponse, MockProvider};
    use futures::StreamExt;

    fn chat(store: &Store) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        store
            .insert_chat(&id, "test", None, None, &now, &now)
            .unwrap();
        id
    }

    fn llm_with_reasoning_tools(
        content: impl Into<String>,
        tool_calls: Vec<LlmToolCall>,
    ) -> Arc<dyn LlmProvider> {
        Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: content.into(),
                tool_calls,
            },
        ))
    }

    #[tokio::test]
    async fn test_reasoning_mode_switch_and_execute() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "I will switch to planning and execute.",
            vec![
                LlmToolCall {
                    id: "tc1".to_string(),
                    name: "set_thinking_mode".to_string(),
                    arguments: serde_json::json!({"mode": "planning"}),
                },
                LlmToolCall {
                    id: "tc2".to_string(),
                    name: "execute".to_string(),
                    arguments: serde_json::json!({}),
                },
            ],
        );
        let harness = Harness::new(store).with_llm(llm).with_reasoning(true);
        let events: Vec<_> = harness
            .run_turn(&chat_id, "build a daily report workflow", "mock", "mock")
            .collect()
            .await;
        assert!(events
            .iter()
            .any(|e| matches!(e, HarnessEvent::ReasoningStarted { mode, .. } if mode == "direct")));
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::ReasoningDone { decision, .. } if decision == "\"execute\"")));
    }

    #[tokio::test]
    async fn test_ask_user_suspends_and_creates_pending_ask() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "Need more info.",
            vec![LlmToolCall {
                id: "tc3".to_string(),
                name: "ask_user".to_string(),
                arguments: serde_json::json!({
                    "question": "Which database should I query?",
                    "quick_replies": ["postgres", "mysql"]
                }),
            }],
        );
        let harness = Harness::new(store.clone())
            .with_llm(llm)
            .with_reasoning(true);
        let events: Vec<_> = harness
            .run_turn(&chat_id, "automate reports", "mock", "mock")
            .collect()
            .await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::AskUser { question, .. } if question == "Which database should I query?")));
        assert!(store.get_pending_ask(&chat_id).unwrap().is_some());

        let llm2 = llm_with_reasoning_tools(
            "Got it, executing now.",
            vec![LlmToolCall {
                id: "tc4".to_string(),
                name: "execute".to_string(),
                arguments: serde_json::json!({}),
            }],
        );
        let harness2 = Harness::new(store.clone()).with_llm(llm2);
        let events2: Vec<_> = harness2
            .resume_turn(&chat_id, "postgres", None, "mock", "mock")
            .collect()
            .await;
        assert!(events2
            .iter()
            .any(|e| matches!(e, HarnessEvent::ReasoningDone { .. })));
        assert!(store.get_pending_ask(&chat_id).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_resume_stores_credential_by_name_not_value() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "Need a token.",
            vec![LlmToolCall {
                id: "tc3".to_string(),
                name: "ask_user".to_string(),
                arguments: serde_json::json!({
                    "question": "What's the GitHub token?",
                    "quick_replies": []
                }),
            }],
        );
        let harness = Harness::new(store.clone()).with_llm(llm).with_reasoning(true);
        let _: Vec<_> = harness
            .run_turn(&chat_id, "set up a deploy", "mock", "mock")
            .collect()
            .await;

        let llm2 = llm_with_reasoning_tools(
            "Got it.",
            vec![LlmToolCall {
                id: "tc4".to_string(),
                name: "execute".to_string(),
                arguments: serde_json::json!({}),
            }],
        );
        let harness2 = Harness::new(store.clone()).with_llm(llm2);
        let _: Vec<_> = harness2
            .resume_turn(
                &chat_id,
                "here you go",
                Some(("github_token".to_string(), "ghs_secret".to_string())),
                "mock",
                "mock",
            )
            .collect()
            .await;

        // The secret is stored by name and only the name enters the transcript.
        assert_eq!(
            store.get_credential("github_token").unwrap(),
            Some("ghs_secret".to_string())
        );
        let rows = store.list_chat_messages(&chat_id).unwrap();
        let user_turn = rows
            .iter()
            .filter(|(_, role, _, _, _)| role == "user")
            .map(|(_, _, content, _, _)| content.clone())
            .find(|c| c.contains("Answer to question"))
            .unwrap_or_default();
        assert!(user_turn.contains("Credential stored as github_token"));
        assert!(user_turn.contains("{{credential:github_token}}"));
        assert!(!user_turn.contains("ghs_secret"));
    }

    #[tokio::test]
    async fn test_auto_approve_skips_ask_user() {
        // With auto-approve on, the harness must not suspend on `ask_user`: it
        // neither emits an AskUser event nor persists a pending ask.
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "Need more info.",
            vec![LlmToolCall {
                id: "tc5".to_string(),
                name: "ask_user".to_string(),
                arguments: serde_json::json!({
                    "question": "Which database should I query?",
                    "quick_replies": ["postgres"]
                }),
            }],
        );
        let harness = Harness::new(store.clone())
            .with_llm(llm)
            .with_reasoning(true)
            .with_auto_approve(true);
        let events: Vec<_> = harness
            .run_turn(&chat_id, "automate reports", "mock", "mock")
            .collect()
            .await;
        assert!(!events.iter().any(|e| matches!(e, HarnessEvent::AskUser { .. })));
        assert!(store.get_pending_ask(&chat_id).unwrap().is_none());
        // The harness records that the question was auto-approved so the next
        // reasoning/execution step sees an answer in history.
        let messages = store.list_chat_messages(&chat_id).unwrap();
        assert!(messages
            .iter()
            .any(|(_, role, content, _, _)| role == "user" && content.contains("auto-approved")));
    }

    /// Records each command line the harness runs, so the approval tests assert
    /// exactly what a decision executed.
    struct RecordingRunner {
        runs: std::sync::Mutex<Vec<String>>,
    }

    impl RecordingRunner {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                runs: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn lines(&self) -> Vec<String> {
            self.runs.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl crate::harness::CommandRunner for RecordingRunner {
        async fn run(&self, command: &str, args: &[String]) -> anyhow::Result<String> {
            let line = if args.is_empty() {
                command.to_string()
            } else {
                format!("{command} {}", args.join(" "))
            };
            self.runs.lock().unwrap().push(line.clone());
            Ok(format!("ran: {line}"))
        }
    }

    /// Drive a turn until its `run_command` call suspends on approval, returning
    /// the store, chat id, recording runner and the events observed so far.
    async fn turn_suspends_on_command() -> (Store, String, Arc<RecordingRunner>, Vec<HarnessEvent>)
    {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "",
            vec![LlmToolCall {
                id: "tc_cmd".to_string(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "echo", "args": ["hi"]}),
            }],
        );
        let runner = RecordingRunner::new();
        let harness = Harness::new(store.clone())
            .with_llm(llm)
            .with_runner(runner.clone())
            .with_workspace_dir("/tmp/workspace");
        let events: Vec<_> = harness
            .run_turn(&chat_id, "run echo hi", "mock", "mock")
            .collect()
            .await;
        (store, chat_id, runner, events)
    }

    /// A `run_command` call suspends before it runs: the harness emits a
    /// `CommandProposed` with the candidate line and the cwd, persists the
    /// proposal, and leaves the call running until a decision arrives.
    #[tokio::test]
    async fn test_command_suspends_before_it_runs() {
        let (store, chat_id, runner, events) = turn_suspends_on_command().await;
        let (id, candidates, cwd) = events
            .iter()
            .find_map(|e| match e {
                HarnessEvent::CommandProposed {
                    id,
                    candidates,
                    cwd,
                } => Some((id.clone(), candidates.clone(), cwd.clone())),
                _ => None,
            })
            .expect("a command tool must suspend on a proposal");
        assert_eq!(id, "tc_cmd");
        assert_eq!(candidates, vec!["echo hi".to_string()]);
        assert_eq!(cwd, "/tmp/workspace");
        assert!(runner.lines().is_empty(), "nothing runs while suspended");
        assert!(store.get_pending_command(&chat_id).unwrap().is_some());
        assert_eq!(
            tool_call_records(&store, &chat_id)[0].status,
            ToolCallStatus::Running
        );
        assert!(!events.iter().any(|e| matches!(e, HarnessEvent::Done)));
    }

    /// Approving runs the chosen command text verbatim.
    #[tokio::test]
    async fn test_approve_runs_chosen_command() {
        let (store, chat_id, runner, _) = turn_suspends_on_command().await;
        let events: Vec<_> = Harness::new(store.clone())
            .with_runner(runner.clone())
            .resume_command(&chat_id, CommandDecision::Approve("echo hi".to_string()))
            .collect()
            .await;
        assert_eq!(runner.lines(), vec!["echo hi".to_string()]);
        assert!(events.iter().any(
            |e| matches!(e, HarnessEvent::ToolCallFinished { id, result } if id == "tc_cmd" && result == "ran: echo hi")
        ));
        assert!(store.get_pending_command(&chat_id).unwrap().is_none());
        let records = tool_call_records(&store, &chat_id);
        assert_eq!(records[0].status, ToolCallStatus::Finished);
        assert_eq!(records[0].result.as_deref(), Some("ran: echo hi"));
    }

    /// Editing runs the edited text instead of the proposal.
    #[tokio::test]
    async fn test_edit_runs_edited_command() {
        let (store, chat_id, runner, _) = turn_suspends_on_command().await;
        let events: Vec<_> = Harness::new(store.clone())
            .with_runner(runner.clone())
            .resume_command(&chat_id, CommandDecision::Edit("echo edited".to_string()))
            .collect()
            .await;
        assert_eq!(runner.lines(), vec!["echo edited".to_string()]);
        assert!(events.iter().any(
            |e| matches!(e, HarnessEvent::ToolCallFinished { id, result } if id == "tc_cmd" && result == "ran: echo edited")
        ));
    }

    /// Rejecting never runs the command; the tool call fails instead.
    #[tokio::test]
    async fn test_reject_errors() {
        let (store, chat_id, runner, _) = turn_suspends_on_command().await;
        let events: Vec<_> = Harness::new(store.clone())
            .with_runner(runner.clone())
            .resume_command(&chat_id, CommandDecision::Reject("too risky".to_string()))
            .collect()
            .await;
        assert!(runner.lines().is_empty(), "a rejected command never runs");
        assert!(events.iter().any(
            |e| matches!(e, HarnessEvent::ToolCallError { id, message } if id == "tc_cmd" && message.contains("rejected by user: too risky"))
        ));
        assert_eq!(
            tool_call_records(&store, &chat_id)[0].status,
            ToolCallStatus::Error
        );
    }

    /// The execution phase streams assistant deltas into a single message row as
    /// they arrive (so the renderer can show the reply progressively), and the
    /// final content is the full concatenation.
    #[tokio::test]
    async fn test_execution_streams_assistant_deltas_into_one_message() {
        use std::pin::Pin;
        use futures::Stream;
        use crate::llm::CompletionResponse;

        struct SplitProvider;
        #[async_trait::async_trait]
        impl LlmProvider for SplitProvider {
            fn name(&self) -> &str {
                "split"
            }
            async fn complete(&self, _req: CompletionRequest) -> anyhow::Result<CompletionResponse> {
                Ok(CompletionResponse {
                    content: "Hello world".to_string(),
                    tool_calls: Vec::new(),
                })
            }
            async fn complete_stream(
                &self,
                _req: CompletionRequest,
            ) -> anyhow::Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
                let events = vec![
                    CompletionStreamEvent::AssistantDelta("Hello ".to_string()),
                    CompletionStreamEvent::AssistantDelta("world".to_string()),
                    CompletionStreamEvent::Done,
                ];
                Ok(Box::pin(futures::stream::iter(events)))
            }
        }

        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let harness = Harness::new(store.clone()).with_llm(Arc::new(SplitProvider));
        let events: Vec<_> = harness
            .run_turn(&chat_id, "hi", "mock", "mock")
            .collect()
            .await;
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::AssistantDelta(d) if d == "Hello ")));
        assert!(events.iter().any(|e| matches!(e, HarnessEvent::AssistantDelta(d) if d == "world")));

        let msgs = store.list_chat_messages(&chat_id).unwrap();
        let assistant = msgs.into_iter().find(|m| m.1 == "assistant").unwrap();
        assert_eq!(assistant.2, "Hello world", "deltas should be concatenated into one assistant message");
    }

    /// Collect the persisted tool-call records from every row of a chat.
    fn tool_call_records(store: &Store, chat_id: &str) -> Vec<ChatToolCall> {
        store
            .list_chat_messages(chat_id)
            .unwrap()
            .into_iter()
            .filter_map(|(_, _, _, tool_calls, _)| tool_calls)
            .flat_map(|json| serde_json::from_str::<Vec<ChatToolCall>>(&json).unwrap_or_default())
            .collect()
    }

    /// A started call is readable from the store while the turn is still running,
    /// and the same row carries the outcome once it finishes.
    #[tokio::test]
    async fn test_tool_call_is_running_in_store_before_turn_ends() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "",
            vec![LlmToolCall {
                id: "tc_live".to_string(),
                name: "credentials".to_string(),
                arguments: serde_json::json!({}),
            }],
        );
        let harness = Harness::new(store.clone()).with_llm(llm);
        let mut events = harness.run_turn(&chat_id, "list credentials", "mock", "mock");

        // Drive the turn only up to the start event: the call must already be
        // persisted as running, well before the turn ends.
        let mut saw_started = false;
        while let Some(event) = events.next().await {
            if matches!(event, HarnessEvent::ToolCallStarted { .. }) {
                saw_started = true;
                break;
            }
        }
        assert!(saw_started, "the turn should have started a tool call");

        let records = tool_call_records(&store, &chat_id);
        assert_eq!(records.len(), 1, "the started call is persisted");
        assert_eq!(records[0].id, "tc_live");
        assert_eq!(records[0].status, ToolCallStatus::Running);
        assert_eq!(records[0].result, None, "a running call has no result yet");

        // Finish the turn; the same row now carries the terminal state.
        let rest: Vec<_> = events.collect().await;
        assert!(rest.iter().any(|e| matches!(e, HarnessEvent::Done)));

        let records = tool_call_records(&store, &chat_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, ToolCallStatus::Finished);
        assert!(records[0].result.is_some(), "a finished call keeps its result");
    }

    /// A call that fails is persisted with the error state and the message.
    #[tokio::test]
    async fn test_failed_tool_call_is_persisted_as_error() {
        let store = Store::open_in_memory().unwrap();
        let chat_id = chat(&store);
        let llm = llm_with_reasoning_tools(
            "",
            vec![LlmToolCall {
                id: "tc_bad".to_string(),
                name: "no_such_tool".to_string(),
                arguments: serde_json::json!({}),
            }],
        );
        let harness = Harness::new(store.clone()).with_llm(llm);
        let events: Vec<_> = harness
            .run_turn(&chat_id, "do the impossible", "mock", "mock")
            .collect()
            .await;
        assert!(events
            .iter()
            .any(|e| matches!(e, HarnessEvent::ToolCallError { .. })));

        let records = tool_call_records(&store, &chat_id);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, ToolCallStatus::Error);
        assert!(records[0]
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("unknown tool"));
    }

    /// A `tool_calls` row written by an older build carries only
    /// `id`/`name`/`arguments`; the new fields must default rather than fail.
    #[test]
    fn test_tool_call_row_from_older_build_still_parses() {
        let old = r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"}}]"#;
        let records: Vec<ChatToolCall> = serde_json::from_str(old).expect("old row must parse");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "ls");
        assert_eq!(records[0].status, ToolCallStatus::Pending);
        assert_eq!(records[0].result, None);
    }
}
