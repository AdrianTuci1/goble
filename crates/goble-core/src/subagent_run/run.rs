use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;

use chrono::Utc;
use futures::FutureExt;
use tokio::sync::oneshot;

use crate::harness::{
    execute_tool_call, harness_tool_definitions, ChatToolCall, ToolCallStatus, ToolTurnContext,
    HARNESS_SYSTEM_PROMPT, SPAWN_SUBAGENT_TOOL,
};
use crate::llm::{CompletionRequest, CompletionResponse, Message, Role, ToolDefinition};
use crate::mcp_manager::McpManager;
use crate::store::Store;
use crate::subagent::{can_spawn_subagent, SubAgentRecord, SubAgentSpec};

use super::host::ChildDeps;
use super::registry::{publish, stop_if_cancelled};

/// The body of a child's task. It owns the record so it can always publish a
/// terminal status: a loop that unwinds leaves `Failed`, never a record that
/// reads as live forever.
pub(super) async fn run_child_task(
    mut record: SubAgentRecord,
    deps: ChildDeps,
    sender: oneshot::Sender<SubAgentRecord>,
) {
    let id = record.spec.id.clone();
    if let Err(payload) = AssertUnwindSafe(run_child_turn(&mut record, &deps))
        .catch_unwind()
        .await
    {
        if !record.status.is_terminal() {
            record.fail(format!(
                "sub-agent {id} panicked: {}",
                panic_message(&payload)
            ));
        }
    }
    deps.host.registry.publish(&record);
    let _ = sender.send(record);
}

/// The text a child unwound with. The payload `catch_unwind` hands back is
/// often a box around the real message (a panic crossing the boxed child future
/// is double-boxed), so peel those levels before giving up.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(inner) = payload.downcast_ref::<Box<dyn std::any::Any + Send>>() {
        return panic_message(inner.as_ref());
    }
    "the child task unwound".to_string()
}

/// The child's tool set: the harness registry, with `spawn_subagent` withheld
/// once the child is at the depth limit — the tool going away is the depth
/// enforcement (S1's `can_spawn_subagent`), never a counter checked after the
/// fact. MCP tools join it the same way they join the parent's list.
pub(crate) fn child_tool_definitions(
    depth: u32,
    store: &Store,
    mcp_manager: &McpManager,
) -> Vec<ToolDefinition> {
    let mut tools = harness_tool_definitions();
    if !can_spawn_subagent(depth) {
        tools.retain(|t| t.name != SPAWN_SUBAGENT_TOOL);
    }
    if let Ok(mcp_tools) = mcp_manager.refresh_from_store(store) {
        tools.extend(mcp_tools);
    }
    tools.push(McpManager::generic_mcp_call_tool());
    tools
}

/// Run one child to a terminal status, updating the record in place: the
/// caller (the child's task, [`run_child_task`]) publishes what it reached.
/// The loop stops at its next check once the child's [`ChildCancel`] has
/// fired, which is how both the parent's `Harness::cancel` bit and a kill of
/// this one child end it `Cancelled` with the reason.
///
/// The future is boxed at this seam: the child's loop awaits `execute_tool_call`
/// again, and an unboxed recursive `async fn` chain has no finite future size
/// (`E0733`).
pub(crate) fn run_child_turn<'a>(
    record: &'a mut SubAgentRecord,
    deps: &'a ChildDeps,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let spec = record.spec.clone();
        let child_chat_id = spec.id.0.clone();
        let tools = child_tool_definitions(spec.depth, &deps.host.store, &deps.host.mcp_manager);

        // The child's prompt is the first row of *its* conversation, under its own
        // id; the parent's transcript only ever sees the tool result.
        if let Err(e) = deps.host.store.insert_chat_message(
            &uuid::Uuid::new_v4().to_string(),
            &child_chat_id,
            "user",
            &spec.prompt,
            None,
            &Utc::now().to_rfc3339(),
        ) {
            record.fail(format!("failed to record the child prompt: {e}"));
            return;
        }

        let mut history = vec![
            Message {
                role: Role::System,
                content: child_system_prompt(&spec),
                tool_calls: None,
                tool_call_id: None,
                tool_status: None,
            },
            Message {
                role: Role::User,
                content: spec.prompt.clone(),
                tool_calls: None,
                tool_call_id: None,
                tool_status: None,
            },
        ];

        loop {
            // Stop before spending another action: the parent's bit or a kill of
            // this child ends the run `Cancelled`, reason included.
            if stop_if_cancelled(record, deps) {
                return;
            }
            // Charge before spending: `Err` means the record is already terminal
            // with the typed reason, so the child ends here instead of looping.
            if record.charge_turn().is_err() {
                publish(record, deps);
                return;
            }
            publish(record, deps);
            let request = CompletionRequest::new(&deps.provider, &deps.model)
                .with_messages(history.clone())
                .with_tools(tools.clone());
            let response = match deps.host.llm.complete(request).await {
                Ok(response) => response,
                Err(e) => {
                    record.fail(format!("sub-agent model call failed: {e}"));
                    return;
                }
            };
            // The call is spent whatever happens next, so its accounting lands
            // before the stop checks: the parent conversation's total carries
            // every child call, cancelled or not.
            if let Some(usage) = response.usage {
                deps.host
                    .registry
                    .note_child_usage(&spec.id.0, &spec.parent_chat_id, &usage);
            }
            // The reply is the last thing a cancelled child would otherwise act
            // on: stopping here keeps a killed run from writing another row.
            if stop_if_cancelled(record, deps) {
                return;
            }
            // The provider's own count replaces the estimate when there is one,
            // in the unit the budget was sized for: tokens the child generated.
            let charge = response
                .usage
                .map_or_else(|| estimate_tokens(&response), |usage| usage.output);
            if record.charge_tokens(charge).is_err() {
                publish(record, deps);
                return;
            }

            let assistant_msg_id = uuid::Uuid::new_v4().to_string();
            let mut call_records: Vec<ChatToolCall> = response
                .tool_calls
                .iter()
                .map(ChatToolCall::planned)
                .collect();
            let tool_calls_json = if call_records.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&call_records).unwrap_or_default())
            };
            if let Err(e) = deps.host.store.insert_chat_message(
                &assistant_msg_id,
                &child_chat_id,
                "assistant",
                &response.content,
                tool_calls_json.as_deref(),
                &Utc::now().to_rfc3339(),
            ) {
                record.fail(format!("failed to record the child reply: {e}"));
                return;
            }
            history.push(Message {
                role: Role::Assistant,
                content: response.content.clone(),
                tool_calls: (!response.tool_calls.is_empty())
                    .then_some(response.tool_calls.clone()),
                tool_call_id: None,
                tool_status: None,
            });

            if response.tool_calls.is_empty() {
                record.complete(response.content);
                return;
            }

            for (index, call) in response.tool_calls.iter().enumerate() {
                if stop_if_cancelled(record, deps) {
                    return;
                }
                if record.charge_tool_call().is_err() {
                    publish(record, deps);
                    return;
                }
                record.set_activity(format!("running {}", call.name));
                publish(record, deps);
                let turn = ToolTurnContext {
                    chat_id: &child_chat_id,
                    provider: &deps.provider,
                    model: &deps.model,
                    depth: spec.depth,
                    host: &deps.host,
                };
                // The child's file tools and commands are scoped to its own cwd,
                // exactly as the parent's are scoped to the workspace dir. A
                // command the child calls runs through the same shared sandboxed
                // runner — a child has no user to approve it, and parking on an
                // approval would be the hang the design forbids.
                let result = execute_tool_call(
                    &deps.host.store,
                    deps.host.runner.as_ref(),
                    deps.host.deploy_sender.as_deref(),
                    &deps.host.mcp_manager,
                    &spec.cwd,
                    &deps.host.docs_dir,
                    call,
                    &deps.web_search,
                    &turn,
                )
                .await;
                let (status, body) = match &result {
                    Ok(value) => (ToolCallStatus::Finished, value.clone()),
                    Err(e) => (ToolCallStatus::Error, e.to_string()),
                };
                if let Err(e) = deps.host.store.insert_chat_message(
                    &uuid::Uuid::new_v4().to_string(),
                    &child_chat_id,
                    "tool",
                    &format!("{}\n{body}", call.id),
                    None,
                    &Utc::now().to_rfc3339(),
                ) {
                    record.fail(format!("failed to record the child tool result: {e}"));
                    return;
                }
                call_records[index].status = status;
                call_records[index].result = Some(body.clone());
                if let Err(e) = deps.host.store.set_chat_message_tool_calls(
                    &assistant_msg_id,
                    &serde_json::to_string(&call_records).unwrap_or_default(),
                ) {
                    record.fail(format!("failed to record the child tool status: {e}"));
                    return;
                }
                history.push(Message {
                    role: Role::Tool,
                    content: body,
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                    tool_status: Some(status),
                });
            }
        }
    })
}

fn child_system_prompt(spec: &SubAgentSpec) -> String {
    format!(
        "You are a sub-agent (type {}) spawned for one routine: {}.\n\
         You work in your own directory ({}) and your own conversation; the agent that spawned you only sees your final reply.\n\
         Use tools until the routine is done, then answer with the result as plain text.\n\n\
         {}",
        spec.subagent_type,
        spec.description,
        spec.cwd.display(),
        HARNESS_SYSTEM_PROMPT
    )
}

/// The child's token charge for one completion, for a provider that reports no
/// usage of its own: an estimate of what the turn generated (the
/// ~4-chars-per-token heuristic). Providers that do report usage are charged
/// their own `output` count instead — the budget counts generated tokens.
fn estimate_tokens(response: &CompletionResponse) -> u64 {
    let mut chars = response.content.chars().count();
    for call in &response.tool_calls {
        chars += call.name.chars().count() + call.arguments.to_string().chars().count();
    }
    chars.div_ceil(4) as u64
}
