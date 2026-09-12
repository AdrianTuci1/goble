use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use futures::{Stream, StreamExt};

use crate::harness::{
    run_approved_command, ChatToolCall, CommandDecision, HarnessEvent, ToolCallStatus,
    WebSearchConfig,
};
use crate::llm::LlmProvider;
use crate::mcp_manager::McpManager;
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::subagent_run::SubAgentHost;
use crate::worker::WorkerId;

use super::mission::run_mission_turn;
use super::persist::persist_tool_call_records;

/// Continue a turn that suspended on an `ask_user`, with the same crate-private
/// host requirement as [`run_mission_turn`].
pub(crate) fn resume_mission_turn(
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
    subagents: SubAgentHost,
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
            subagents,
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
    // The row is the call id plus the body; the outcome is `status`, persisted
    // on the call record below, so it is never encoded into the result text.
    let text = format!("{call_id}\n{body}");
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
