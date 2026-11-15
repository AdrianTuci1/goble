//! Auto-compaction of a conversation that outgrows its model's window.
//!
//! The transcript is never rewritten: every message row stays in the store, and
//! the app keeps drawing the whole conversation. What changes is the request:
//! the model summarises the turns before the current prompt, the summary is
//! recorded with the message it bounds, and [`super::history::build_history`]
//! reads that summary plus the rows after the boundary instead of the whole
//! transcript.

use anyhow::{bail, Result};
use chrono::Utc;

use crate::agent_memory::{CompactionResult, COMPACTION_PROMPT};
use crate::harness::HarnessEvent;
use crate::llm::{CompletionRequest, LlmProvider, Message, Role};
use crate::store::Store;

/// How full the window is before a turn compacts its history. Grok's own
/// threshold is in the same range; the point is to leave room for the reply
/// and the turn's tool calls.
pub(super) const COMPACT_AT_PERCENT: u64 = 80;

/// The most bytes one request may carry before the turn compacts its history.
///
/// The endpoint's own body cap is not the model's context window: DeepSeek
/// answered 413 Request Entity Too Large (an openresty page, so the cap is the
/// endpoint's, not this adapter's) to a `deepseek-flash` conversation whose
/// window is 256k tokens and which one 56 MB tool result had put far past any
/// body cap. The token trigger only sees what the provider priced, so a
/// request can be refused long before it reaches 80% of the window. The
/// serialized request is measured as well; 900 KB leaves the reply and a tool
/// call room under a hard 1 MB cap.
pub(crate) const MAX_REQUEST_BYTES: usize = 900 * 1024;

/// What a compaction can know about why it runs.
///
/// The token trigger knows how full the window is. The payload trigger only
/// knows the request outgrew the provider's body cap, which happens long
/// before the window fills: its percentage is of the cap, and the notice says
/// so rather than reporting a window fullness nobody measured.
pub(crate) enum CompactionTrigger {
    /// The prompt filled this share of the model's window.
    Window { window: u64, used: u64 },
    /// The endpoint refused the request's body for its size, or the body as
    /// serialized here passed the cap this adapter measures it against.
    Payload { bytes: usize, cap: usize },
}

/// The bytes a request puts on the wire, near enough to measure the
/// provider's body cap: the messages and tool definitions serialized as the
/// OpenAI adapter builds them, so content with its JSON escaping, tool calls
/// with their arguments and a replayed chain of thought are all counted.
pub(crate) fn request_bytes(request: &CompletionRequest) -> usize {
    serialized_bytes(&request.messages) + serialized_bytes(&request.tools)
}

/// How many bytes `value` takes as JSON, counted without building the string:
/// a request near the cap is megabytes long, and measuring it must not copy
/// them.
fn serialized_bytes(value: &impl serde::Serialize) -> usize {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    match serde_json::to_writer(&mut counter, value) {
        Ok(()) => counter.0,
        Err(_) => 0,
    }
}

/// The payload trigger for a request that passes the provider's body cap,
/// `None` for one that fits.
pub(crate) fn payload_trigger(request: &CompletionRequest) -> Option<CompactionTrigger> {
    let bytes = request_bytes(request);
    (bytes > MAX_REQUEST_BYTES).then_some(CompactionTrigger::Payload {
        bytes,
        cap: MAX_REQUEST_BYTES,
    })
}

/// The most transcript text one summarisation call is given. A conversation
/// near its window is larger than the compaction prompt budget, so the oldest
/// turns are dropped from the input: the summary is of what the model can still
/// read, not of text the summariser would itself overflow on.
const COMPACTION_INPUT_CHARS: usize = 240_000;

/// What one successful compaction leaves behind.
pub(super) struct CompactionOutcome {
    /// The last message the summary covers; history resumes after it.
    pub boundary_message_id: String,
    pub summary: String,
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub message_count: usize,
}

/// What one compaction attempt did, as the turn streams it: the events the
/// transcript's notice and the app's refresh read, and the token count the
/// conversation continues from. `after_tokens` stays `None` when the attempt
/// failed, so the turn does not try again in the same state.
pub(crate) struct CompactionRun {
    pub events: Vec<HarnessEvent>,
    pub after_tokens: Option<u64>,
}

/// Compact once. The transcript gets a notice row that is rewritten from
/// "Compacting…" to the outcome, so the user sees the work where it happened;
/// a failure leaves the turn running, with the row saying so instead of an
/// error ending the reply. `trigger` is what the notice and the events say the
/// conversation outgrew: the window, or the provider's body cap.
pub(crate) async fn compact(
    store: &Store,
    chat_id: &str,
    llm: &dyn LlmProvider,
    provider: &str,
    model: &str,
    trigger: CompactionTrigger,
) -> CompactionRun {
    // What the notice says while it runs, and the reason it adds to the
    // outcome: the token counts bracket every compaction, but only the payload
    // trigger has a body cap to name.
    let (percentage, used, started, reason) = match trigger {
        CompactionTrigger::Window { window, used } => {
            let percentage = percentage_of(used, window);
            (
                percentage,
                used,
                format!("Context {percentage}% full. Compacting…"),
                None,
            )
        }
        CompactionTrigger::Payload { bytes, cap } => {
            let percentage = percentage_of(bytes as u64, cap as u64);
            // The provider never priced a request it refused, so the token
            // count this compaction starts from is the stored rows' estimate.
            let used = store
                .list_chat_messages(chat_id)
                .map(|rows| stored_rows_tokens(&rows))
                .unwrap_or(0);
            let quoted = format!("{} against the {} cap", kilobytes(bytes), kilobytes(cap));
            (
                percentage,
                used,
                format!("Request body {quoted}. Compacting…"),
                Some(format!("request body: {quoted}")),
            )
        }
    };
    let notice_id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    if let Err(e) = store.insert_chat_message(&notice_id, chat_id, "notice", &started, None, &now) {
        log::warn!("could not write the compaction notice: {e}");
        return CompactionRun {
            events: Vec::new(),
            after_tokens: None,
        };
    }
    let mut events = vec![HarnessEvent::ContextCompaction {
        chat_id: chat_id.to_string(),
        phase: "started".to_string(),
        percentage,
        before: used,
        after: 0,
    }];
    match summarize(store, chat_id, llm, provider, model, used).await {
        Ok(outcome) => {
            let id = uuid::Uuid::new_v4().to_string();
            if let Err(e) = store.insert_chat_compaction(
                &id,
                chat_id,
                &outcome.boundary_message_id,
                &outcome.summary,
                outcome.before_tokens,
                outcome.after_tokens,
                outcome.message_count,
                &Utc::now().to_rfc3339(),
            ) {
                log::warn!("could not record the compaction: {e}");
            }
            events.push(HarnessEvent::ContextCompaction {
                chat_id: chat_id.to_string(),
                phase: "completed".to_string(),
                percentage,
                before: outcome.before_tokens,
                after: outcome.after_tokens,
            });
            let mut outcome_note = format!(
                "Context compacted: {} → {} tokens",
                thousands(outcome.before_tokens),
                thousands(outcome.after_tokens)
            );
            if let Some(reason) = &reason {
                outcome_note.push_str(&format!(" ({reason})"));
            }
            let _ = store.set_chat_message_content(&notice_id, &outcome_note);
            CompactionRun {
                events,
                after_tokens: Some(outcome.after_tokens),
            }
        }
        Err(e) => {
            log::warn!("compaction failed: {e}");
            events.push(HarnessEvent::ContextCompaction {
                chat_id: chat_id.to_string(),
                phase: "failed".to_string(),
                percentage,
                before: used,
                after: used,
            });
            let _ = store.set_chat_message_content(&notice_id, "Compaction failed.");
            CompactionRun {
                events,
                after_tokens: None,
            }
        }
    }
}

/// A token count with thousands separators, as the notice row reads it.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `part` as a percentage of `whole`, capped at 100. A zero `whole` has no
/// percentage to report, so it reads as 0 instead of being divided by.
fn percentage_of(part: u64, whole: u64) -> u8 {
    if whole == 0 {
        return 0;
    }
    part.saturating_mul(100)
        .checked_div(whole)
        .unwrap_or(100)
        .min(100) as u8
}

/// Bytes as the notice reads them, in whole kilobytes: the unit the model's
/// configuration quotes the cap in.
fn kilobytes(bytes: usize) -> String {
    format!("{} KB", (bytes as u64).div_ceil(1024))
}

/// A rough token count for text the provider never priced: four characters per
/// token is the usual English and source-code ratio, and a compaction only
/// needs to be right about whether it crossed the window, not to the token.
pub(super) fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// How many tokens a conversation's stored rows occupy, approximated the same
/// way. Used before a turn's first call, when no provider count exists yet, and
/// by a child's loop to know how far its own conversation has grown since the
/// last compaction.
pub(crate) fn stored_rows_tokens(rows: &[(String, String, String, Option<String>, String)]) -> u64 {
    rows.iter()
        .map(|(_, _, content, tool_calls, _)| {
            let calls = tool_calls.as_deref().map(str::len).unwrap_or(0) as u64;
            estimate_tokens(content) + calls.div_ceil(4)
        })
        .sum()
}

/// Summarise everything before the conversation's current prompt. The prompt
/// itself, and any tool rows the current turn produced, stay in the request.
///
/// A sub-agent's conversation has exactly one prompt, the row it was spawned
/// with, and no later one to keep: its transcript keeps the last exchange
/// instead — the assistant turn and the tool results that answer it — which is
/// what the child's next request replays. A parent turn that outgrew the cap
/// before its second prompt is compacted the same way.
pub(super) async fn summarize(
    store: &Store,
    chat_id: &str,
    llm: &dyn LlmProvider,
    provider: &str,
    model: &str,
    before_tokens: u64,
) -> Result<CompactionOutcome> {
    let rows = store.list_chat_messages(chat_id)?;
    let keep_from = rows
        .iter()
        .rposition(|(_, role, ..)| role == "user")
        .filter(|index| *index > 0)
        .or_else(|| {
            rows.iter()
                .rposition(|(_, role, ..)| role == "assistant" || role == "tool_calls")
        })
        .ok_or_else(|| anyhow::anyhow!("nothing to compact yet"))?;
    if keep_from == 0 {
        bail!("nothing before the current turn to compact");
    }
    let boundary_message_id = rows[keep_from - 1].0.clone();
    let older = &rows[..keep_from];
    let mut transcript = String::new();
    for (_, role, content, _, _) in older {
        transcript.push_str(role);
        transcript.push_str(": ");
        transcript.push_str(content);
        transcript.push('\n');
    }
    let transcript = tail(&transcript, COMPACTION_INPUT_CHARS);
    let request = CompletionRequest::new(provider, model)
        .with_system(COMPACTION_PROMPT)
        .with_messages(vec![Message {
            role: Role::User,
            content: format!("Conversation so far:\n{transcript}"),
            tool_calls: None,
            tool_call_id: None,
            tool_status: None,
            reasoning_content: None,
        }]);
    let response = llm.complete(request).await?;
    let summary = parse_summary(&response.content);
    if summary.trim().is_empty() {
        bail!("the summariser returned nothing");
    }
    let after_tokens =
        estimate_tokens(&summary) + stored_rows_tokens(rows.get(keep_from..).unwrap_or_default());
    Ok(CompactionOutcome {
        boundary_message_id,
        summary,
        before_tokens,
        after_tokens,
        message_count: older.len(),
    })
}

/// The last `limit` characters of `text`, on a character boundary, with the cut
/// marked so the summariser knows the transcript it reads is a tail.
fn tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - limit).collect();
    format!("[… earlier text omitted …]\n{tail}")
}

/// The summary from a compaction reply. The prompt asks for a JSON object, but
/// a model that answers in prose still gives a usable summary: the compaction
/// saves context either way, and a parse failure must not fail the turn.
fn parse_summary(content: &str) -> String {
    let trimmed = content.trim();
    let unfenced = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.strip_suffix("```"))
        .map(|block| {
            block
                .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == '\n')
                .trim()
        })
        .unwrap_or(trimmed);
    if let Ok(result) = serde_json::from_str::<CompactionResult>(unfenced) {
        let mut summary = result.summary.trim().to_string();
        if !result.next_steps.is_empty() {
            summary.push_str("\nNext steps: ");
            summary.push_str(&result.next_steps.join("; "));
        }
        if !summary.is_empty() {
            return summary;
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_read_from_the_prompts_json() {
        let content = r#"{"session_id":"s","summary":"The migration was audited.","facts":[],"decisions":[],"goals_completed":[],"new_goals":[],"milestones_completed":[],"next_steps":["Ship the fix"],"message_count":12}"#;
        assert_eq!(
            parse_summary(content),
            "The migration was audited.\nNext steps: Ship the fix"
        );
    }

    #[test]
    fn a_fenced_summary_is_read_too() {
        let content = "```json\n{\"session_id\":\"s\",\"summary\":\"Done.\",\"facts\":[],\"decisions\":[],\"goals_completed\":[],\"new_goals\":[],\"milestones_completed\":[],\"next_steps\":[],\"message_count\":2}\n```";
        assert_eq!(parse_summary(content), "Done.");
    }

    #[test]
    fn a_prose_reply_still_becomes_a_summary() {
        assert_eq!(parse_summary("We did a lot."), "We did a lot.");
    }

    #[test]
    fn a_cut_keeps_the_tail_and_says_so() {
        let text: String = (0..10).map(|i| i.to_string()).collect();
        assert_eq!(tail(&text, 4), "[… earlier text omitted …]\n6789");
        assert_eq!(tail(&text, 100), text);
    }

    fn seeded_store() -> Store {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now().to_rfc3339();
        store.insert_chat("c", "t", None, None, &now, &now).unwrap();
        store
            .insert_chat_message("m1", "c", "user", "the first goal", None, &now)
            .unwrap();
        store
            .insert_chat_message("m2", "c", "assistant", "the first reply", None, &now)
            .unwrap();
        store
            .insert_chat_message("m3", "c", "user", "the current prompt", None, &now)
            .unwrap();
        store
    }

    fn summariser() -> crate::llm::MockProvider {
        crate::llm::MockProvider::new(
            "mock",
            crate::llm::CompletionResponse::new(
                r#"{"session_id":"c","summary":"The first turns.","facts":[],"decisions":[],"goals_completed":[],"new_goals":[],"milestones_completed":[],"next_steps":[],"message_count":2}"#,
                Vec::new(),
            ),
        )
    }

    #[tokio::test]
    async fn a_full_window_summarises_the_turns_before_the_prompt() {
        let store = seeded_store();
        let llm = summariser();
        let run = compact(
            &store,
            "c",
            &llm,
            "mock",
            "mock",
            CompactionTrigger::Window {
                window: 1000,
                used: 900,
            },
        )
        .await;
        assert!(run.after_tokens.is_some(), "a good summary completes");
        assert_eq!(run.events.len(), 2);
        assert!(matches!(
            &run.events[0],
            HarnessEvent::ContextCompaction { phase, percentage, before, .. }
                if phase == "started" && *percentage == 90 && *before == 900
        ));

        let compaction = store.latest_chat_compaction("c").unwrap().unwrap();
        assert_eq!(compaction.boundary_message_id, "m2");
        assert_eq!(compaction.summary, "The first turns.");
        assert_eq!(compaction.before_tokens, 900);
        assert_eq!(compaction.message_count, 2);

        let rows = store.list_chat_messages("c").unwrap();
        let notice = rows
            .iter()
            .find(|(_, role, ..)| role == "notice")
            .expect("the notice row is written");
        assert!(
            notice.2.starts_with("Context compacted: 900 → "),
            "the start notice is rewritten to the outcome, got {:?}",
            notice.2
        );
        assert!(
            !notice.2.contains("Compacting"),
            "the outcome replaces the in-progress words"
        );
    }

    #[tokio::test]
    async fn a_compacted_history_keeps_only_the_current_prompt_and_the_summary() {
        let store = seeded_store();
        let llm = summariser();
        let _ = compact(
            &store,
            "c",
            &llm,
            "mock",
            "mock",
            CompactionTrigger::Window {
                window: 1000,
                used: 900,
            },
        )
        .await;
        let mission = crate::reasoning::MissionState {
            id: "m".to_string(),
            chat_id: "c".to_string(),
            goal: "goal".to_string(),
            status: "active".to_string(),
            plan: None,
            workflow_id: None,
            reasoning_steps: Vec::new(),
            pending_ask: None,
        };
        let history = super::super::history::build_history(
            &store,
            "c",
            &mission,
            &[],
            &[],
            crate::harness::ThinkingMode::Direct,
        )
        .await;
        assert_eq!(history.len(), 2, "one summary plus the current prompt");
        assert_eq!(history[0].role, Role::System);
        assert!(
            history[0].content.contains("The first turns."),
            "the model reads its own summary, got {:?}",
            history[0].content
        );
        assert_eq!(history[1].content, "the current prompt");
        assert!(
            !history.iter().any(|m| m.content == "the first goal"),
            "the summarised turns leave the request"
        );
        assert!(
            !history.iter().any(|m| m.content.contains("Compacting")),
            "a notice is never sent to the model"
        );
    }

    #[tokio::test]
    async fn a_failed_summary_keeps_the_turn_alive_and_says_so() {
        let store = seeded_store();
        let llm = crate::llm::MockProvider::new(
            "mock",
            crate::llm::CompletionResponse::new("", Vec::new()),
        );
        let run = compact(
            &store,
            "c",
            &llm,
            "mock",
            "mock",
            CompactionTrigger::Window {
                window: 1000,
                used: 900,
            },
        )
        .await;
        assert_eq!(run.after_tokens, None);
        assert!(matches!(
            &run.events[1],
            HarnessEvent::ContextCompaction { phase, .. } if phase == "failed"
        ));
        let rows = store.list_chat_messages("c").unwrap();
        assert!(
            rows.iter()
                .any(|(_, role, content, ..)| role == "notice" && content == "Compaction failed."),
            "the notice states the failure instead of ending the turn"
        );
        assert!(store.latest_chat_compaction("c").unwrap().is_none());
    }

    #[test]
    fn the_request_size_counts_content_calls_and_tools() {
        let request = CompletionRequest::new("mock", "mock")
            .with_messages(vec![Message {
                role: Role::Tool,
                content: "x".repeat(100),
                tool_calls: Some(vec![crate::llm::LlmToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "/tmp/a"}),
                }]),
                tool_call_id: Some("call_1".to_string()),
                tool_status: None,
                reasoning_content: None,
            }])
            .with_tools(vec![crate::llm::ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file.".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }]);
        let bytes = request_bytes(&request);
        assert_eq!(
            bytes,
            serde_json::to_string(&request.messages).unwrap().len()
                + serde_json::to_string(&request.tools).unwrap().len(),
            "the count is the serialized shape, line for line"
        );
        assert!(bytes > 100, "the content is counted: {bytes}");
        assert!(
            bytes > request.messages[0].content.len() + 40,
            "the call, its arguments and the tool definition are counted too: {bytes}"
        );
        // A small request is nowhere near the cap, so no compaction is asked for.
        assert!(payload_trigger(&request).is_none());
    }

    #[tokio::test]
    async fn a_payload_triggered_compaction_says_the_body_passed_the_cap() {
        let store = seeded_store();
        let llm = summariser();
        let run = compact(
            &store,
            "c",
            &llm,
            "mock",
            "mock",
            CompactionTrigger::Payload {
                bytes: MAX_REQUEST_BYTES + 1024,
                cap: MAX_REQUEST_BYTES,
            },
        )
        .await;
        assert!(run.after_tokens.is_some(), "the summary completes");
        assert!(matches!(
            &run.events[0],
            HarnessEvent::ContextCompaction { phase, percentage, before, .. }
                if phase == "started" && *percentage == 100 && *before > 0
        ));
        let rows = store.list_chat_messages("c").unwrap();
        let notice = rows
            .iter()
            .find(|(_, role, ..)| role == "notice")
            .expect("the notice row is written");
        assert!(
            notice.2.contains("against the 900 KB cap"),
            "the notice names the body cap, not a window fullness: {:?}",
            notice.2
        );
        // The outcome row still reports the token counts on either side.
        assert!(notice.2.contains("Context compacted:"), "{:?}", notice.2);
    }

    /// A sub-agent's chat holds one prompt, the row it was spawned with: there
    /// is no later prompt to keep, so the compaction keeps the last exchange.
    #[tokio::test]
    async fn a_conversation_with_one_prompt_keeps_its_last_exchange() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now().to_rfc3339();
        store
            .insert_chat("child", "t", None, None, &now, &now)
            .unwrap();
        for (id, role, content) in [
            ("m1", "user", "review it"),
            ("m2", "assistant", "read the diff"),
            ("m3", "assistant", "writing notes"),
        ] {
            store
                .insert_chat_message(id, "child", role, content, None, &now)
                .unwrap();
        }
        let run = compact(
            &store,
            "child",
            &summariser(),
            "mock",
            "mock",
            CompactionTrigger::Payload {
                bytes: MAX_REQUEST_BYTES + 1,
                cap: MAX_REQUEST_BYTES,
            },
        )
        .await;
        assert!(run.after_tokens.is_some());
        let compaction = store.latest_chat_compaction("child").unwrap().unwrap();
        assert_eq!(
            compaction.boundary_message_id, "m2",
            "the last exchange stays in the request, the turns before it do not"
        );
        assert_eq!(compaction.message_count, 2);
    }

    #[tokio::test]
    async fn a_conversation_with_nothing_before_the_turn_is_not_compacted() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now().to_rfc3339();
        store.insert_chat("c", "t", None, None, &now, &now).unwrap();
        store
            .insert_chat_message("m1", "c", "user", "the first prompt", None, &now)
            .unwrap();
        let run = compact(
            &store,
            "c",
            &summariser(),
            "mock",
            "mock",
            CompactionTrigger::Payload {
                bytes: MAX_REQUEST_BYTES + 1,
                cap: MAX_REQUEST_BYTES,
            },
        )
        .await;
        assert_eq!(run.after_tokens, None);
        assert!(matches!(
            &run.events[1],
            HarnessEvent::ContextCompaction { phase, .. } if phase == "failed"
        ));
        assert!(store.latest_chat_compaction("c").unwrap().is_none());
    }
}
