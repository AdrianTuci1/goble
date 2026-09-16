use super::*;


/// Extract the text of a chat fragment, for link scanning. Non-text fragments
/// (lists, actions, terminal blocks) contribute nothing.
pub(crate) fn fragment_text(f: &goble_ui::elements::chat_content::ChatFragment) -> Option<&str> {
    use goble_ui::elements::chat_content::ChatFragmentKind;
    match &f.kind {
        ChatFragmentKind::Text(s)
        | ChatFragmentKind::Bold(s)
        | ChatFragmentKind::Italic(s)
        | ChatFragmentKind::BoldItalic(s)
        | ChatFragmentKind::Code(s) => Some(s.as_str()),
        ChatFragmentKind::CodeBlock { code, .. } => Some(code.as_str()),
        ChatFragmentKind::Heading { text, .. } => Some(text.as_str()),
        ChatFragmentKind::Link { url, .. } => Some(url.as_str()),
        _ => None,
    }
}

/// Scan messages (most-recent assistant content first) for a BYOH desktop
/// handoff URI, so a harness that only prints a hyperlink still surfaces a
/// clickable handoff affordance.
pub(crate) fn message_screen_link(messages: &[ChatMessage]) -> Option<String> {
    for msg in messages.iter().rev() {
        if msg.role != ChatRole::Assistant {
            continue;
        }
        let mut plain = String::new();
        for f in &msg.fragments {
            if let Some(t) = fragment_text(f) {
                plain.push_str(t);
                plain.push(' ');
            }
        }
        if let Some(link) = detect_screen_link(&plain) {
            return Some(link);
        }
    }
    None
}

/// Format an RFC3339 timestamp as a short relative "time ago" label (e.g.
/// "40 min ago"). Falls back to the raw string when it cannot be parsed.
pub(crate) fn time_ago(updated_at: &str) -> String {
    match DateTime::parse_from_rfc3339(updated_at) {
        Ok(ts) => {
            let now = Utc::now();
            let dur = now.signed_duration_since(ts.with_timezone(&Utc));
            let secs = dur.num_seconds().max(0);
            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                format!("{} min ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else if secs < 172800 {
                "Yesterday".to_string()
            } else {
                format!("{} days ago", secs / 86400)
            }
        }
        Err(_) => updated_at.to_string(),
    }
}

/// Short human label for a workflow [`Trigger`], e.g. `cron 0 12 * * *`.
pub(crate) fn trigger_label(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Manual => "manual".to_string(),
        Trigger::Cron { expression } => format!("cron {expression}"),
        Trigger::Http { path } => format!("http {path}"),
        Trigger::Heartbeat { interval_seconds } => format!("heartbeat {interval_seconds}s"),
    }
}

/// The output a stored `role="tool"` row carries. The harness writes a tool
/// result as `"<call_id>\n<output>"`, so the first line names the call the body
/// belongs to and everything after it is the output.
pub(crate) fn tool_result_body(content: &str) -> String {
    content
        .splitn(2, '\n')
        .nth(1)
        .unwrap_or("")
        .trim_end()
        .to_string()
}

/// The live turn-status footer's state for one pane, read from the pane's own
/// runtime plus the workspace's running executions and the pane's own live
/// sub-agent children (C1's live data).
///
/// The busy activity is resolved from what is genuinely under way: a pending
/// approval, a pending question, an in-flight tool call — the row its own family
/// gives it, or the child it is waiting on when the call spawned one — or, with
/// none of those, the model's phase, read from the reasoning rows this turn has
/// produced: none yet means the model has not started streaming, an open newest
/// row means it is still reasoning, a closed one means it is writing. `elapsed`
/// is the age of the observed turn start; a turn whose start was not observed
/// reports `None` rather than an invented time.
pub(crate) fn pane_turn_status(
    rt: Option<&PaneRuntime>,
    executions: usize,
    sub_agents: usize,
) -> TurnStatus {
    let Some(rt) = rt else {
        // A pane with no runtime yet has no turn of its own; the workspace's
        // running executions are still in flight.
        return still_running_status(0, executions, sub_agents);
    };
    let in_flight = rt.in_flight_tools.len();
    if rt.busy {
        let activity = if rt.pending_command.is_some() {
            TurnActivity::WaitingOnApproval
        } else if rt.pending_ask.is_some() {
            TurnActivity::WaitingOnQuestion
        } else if let Some(call) = rt.in_flight_tools.values().min_by(|a, b| a.id.cmp(&b.id)) {
            // A spawn is awaited: while its call is in flight the turn is doing
            // nothing but waiting on the child, so the child is what it names.
            if tool_kind_for(&call.name) == ToolKind::SubAgent {
                let record = rt
                    .sub_agents
                    .values()
                    .find(|record| record.parent_call_id == call.id);
                TurnActivity::WaitingOnSubAgent {
                    description: record.map(|record| record.row.description.clone()),
                    activity: record
                        .and_then(|record| record.row.activity_segment().map(str::to_string)),
                }
            } else {
                TurnActivity::Tool {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                }
            }
        } else if rt.reasoning.is_empty() {
            TurnActivity::WaitingForResponse
        } else if rt.reasoning.last().map(|row| !row.done).unwrap_or(false) {
            TurnActivity::Thinking
        } else {
            TurnActivity::Responding
        };
        return TurnStatus::Busy {
            activity,
            elapsed: rt.turn_started_at.map(|started| started.elapsed()),
            work_count: in_flight + executions,
        };
    }

    still_running_status(in_flight, executions, sub_agents)
}

/// The idle-with-work state: one entry per kind that is really running, in the
/// order Command, Execution, SubAgent, or [`TurnStatus::Idle`] (zero height)
/// when nothing is.
pub(crate) fn still_running_status(
    in_flight: usize,
    executions: usize,
    sub_agents: usize,
) -> TurnStatus {
    let mut kinds = Vec::new();
    if in_flight > 0 {
        kinds.push(WorkKindCount {
            kind: WorkKind::Command,
            count: in_flight,
        });
    }
    if executions > 0 {
        kinds.push(WorkKindCount {
            kind: WorkKind::Execution,
            count: executions,
        });
    }
    if sub_agents > 0 {
        kinds.push(WorkKindCount {
            kind: WorkKind::SubAgent,
            count: sub_agents,
        });
    }
    if kinds.is_empty() {
        TurnStatus::Idle
    } else {
        TurnStatus::StillRunning { kinds }
    }
}

/// The call id a `role="tool"` row belongs to: the harness writes the result
/// row as `"<call_id>\n<output>"`, so the first line names the call. It is what
/// lets the row be folded into the call it belongs to instead of drawn as a
/// segment of its own.
pub(crate) fn tool_result_call_id(content: &str) -> &str {
    content.split('\n').next().unwrap_or("").trim()
}

/// One store row plus the message it parsed into. Remembered per row id so a
/// refresh can tell an unchanged row from an edited one.
#[derive(Clone, Debug)]
pub(crate) struct CachedMessage {
    role: String,
    content: String,
    tool_calls: Option<String>,
    message: ChatMessage,
}

impl CachedMessage {
    /// Whether `row` still carries the fields this entry was parsed from. An
    /// edit to any of them (a streaming delta appends to `content`) invalidates
    /// the entry so the message is re-parsed.
    pub(crate) fn matches(&self, row: &goble_desktop_service::ChatMessage) -> bool {
        self.role == row.role && self.content == row.content && self.tool_calls == row.tool_calls
    }
}

/// Parse-once-per-change cache for one pane's transcript.
///
/// Each store row is parsed into a [`ChatMessage`] once and remembered under its
/// row id together with the row fields it was parsed from. A refresh re-parses
/// only the rows whose content (or role / tool calls) changed and reuses every
/// other message, so a frame that carries no new delta does no parsing at all.
/// Entries for rows that disappeared are dropped.
#[derive(Clone, Debug, Default)]
pub struct MessageParseCache {
    entries: HashMap<String, CachedMessage>,
    parses: u64,
}

impl MessageParseCache {
    /// Build the transcript for `rows` (in store order), re-parsing only the
    /// rows whose content changed since the previous call.
    ///
    /// A `role="tool"` row whose call a preceding row already carries is folded
    /// into that call rather than becoming a message of its own: the call's
    /// output is that row's body, and the agent's reply is where it is read. So
    /// an agent's call is one continuous row in its reply and never a terminal
    /// block — a block belongs to a command the user ran, which the pane builds.
    pub fn resolve(&mut self, rows: &[goble_desktop_service::ChatMessage]) -> Vec<ChatMessage> {
        let mut messages: Vec<ChatMessage> = Vec::with_capacity(rows.len());
        let mut next: HashMap<String, CachedMessage> = HashMap::with_capacity(rows.len());
        // Where each persisted call was drawn, by call id: the index of the
        // message carrying it and of the call inside that message.
        let mut call_sites: HashMap<String, (usize, usize)> = HashMap::new();
        for row in rows {
            let call_id = (row.role == "tool").then(|| tool_result_call_id(&row.content));
            if let Some((message_index, call_index)) =
                call_id.and_then(|id| call_sites.get(id).copied())
            {
                let call = &mut messages[message_index].tool_calls[call_index];
                let body = tool_result_body(&row.content);
                if !body.is_empty() && call.result.as_deref().unwrap_or("").is_empty() {
                    call.result = Some(body);
                }
                continue;
            }
            let message = match self.entries.get(&row.id) {
                Some(cached) if cached.matches(row) => cached.message.clone(),
                _ => {
                    self.parses += 1;
                    parse_chat_row(row)
                }
            };
            for (call_index, call) in message.tool_calls.iter().enumerate() {
                call_sites.insert(call.id.clone(), (messages.len(), call_index));
            }
            next.insert(
                row.id.clone(),
                CachedMessage {
                    role: row.role.clone(),
                    content: row.content.clone(),
                    tool_calls: row.tool_calls.clone(),
                    message: message.clone(),
                },
            );
            messages.push(message);
        }
        self.entries = next;
        messages
    }

    /// How many rows this cache has parsed. Test observability for the
    /// parse-once-per-change contract; not read by the app.
    pub fn parse_count(&self) -> u64 {
        self.parses
    }
}

/// Convert a live `chat:tool` event into the renderable [`ToolCall`] the
/// transcript overlay uses.
pub(crate) fn tool_call_from_event(call: &goble_desktop_service::ToolCallEvent) -> ToolCall {
    ToolCall {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
        status: call.status,
        result: call.result.clone(),
    }
}

/// Overlay a pane's in-flight calls on the transcript built from persisted
/// rows: a call already present is updated to its live record, and a call not
/// yet persisted is attached to the trailing assistant message (or a fresh one)
/// so it renders while it runs. Only running calls are held, so a persisted
/// terminal state is never overwritten.
///
/// A live record that carries no result yet keeps the one the transcript
/// already had, so the output folded in from a persisted result row is not lost
/// for the rest of the call's run.
pub(crate) fn overlay_in_flight(messages: &mut Vec<ChatMessage>, in_flight: &HashMap<String, ToolCall>) {
    if in_flight.is_empty() {
        return;
    }
    for call in in_flight.values() {
        if let Some(existing) = messages
            .iter_mut()
            .flat_map(|m| m.tool_calls.iter_mut())
            .find(|c| c.id == call.id)
        {
            let carried = call
                .result
                .is_none()
                .then(|| existing.result.clone())
                .flatten();
            *existing = call.clone();
            if existing.result.is_none() {
                existing.result = carried;
            }
            continue;
        }
        match messages
            .iter_mut()
            .rev()
            .find(|m| m.role == ChatRole::Assistant)
        {
            Some(message) => message.tool_calls.push(call.clone()),
            None => {
                let mut message = ChatMessage::new(ChatRole::Assistant, Vec::new());
                message.tool_calls.push(call.clone());
                messages.push(message);
            }
        }
    }
}

/// Overlay the pane's accumulated reasoning steps on the transcript: they are
/// pulled out of and re-inserted into the trailing assistant message so each
/// step renders as its own row ahead of that message's prose. Re-running is
/// idempotent, so a refresh or a live delta never duplicates a row.
pub(crate) fn overlay_reasoning(messages: &mut Vec<ChatMessage>, reasoning: &[ReasoningRow]) {
    if reasoning.is_empty() {
        return;
    }
    let index = match messages.iter().rposition(|m| m.role == ChatRole::Assistant) {
        Some(index) => index,
        None => {
            messages.push(ChatMessage::new(ChatRole::Assistant, Vec::new()));
            messages.len() - 1
        }
    };
    let message = &mut messages[index];
    message.fragments.retain(|f| {
        !matches!(
            f.kind,
            goble_ui::elements::chat_content::ChatFragmentKind::Reasoning { .. }
        )
    });
    let mut fragments: Vec<ChatFragment> = reasoning
        .iter()
        .map(|row| {
            ChatFragment::reasoning(
                reasoning_row_key(&row.chat_id, row.step),
                row.mode.clone(),
                row.text.clone(),
                row.done,
            )
        })
        .collect();
    fragments.extend(std::mem::take(&mut message.fragments));
    message.fragments = fragments;
}

/// The app-owned expand-state key for one reasoning step. It is stable while
/// the step's text streams, so a row the user opened stays open.
pub(crate) fn reasoning_row_key(chat_id: &str, step: usize) -> String {
    format!("{chat_id}:{step}")
}

/// Parse one stored row into a [`ChatMessage`]: Markdown, with tool-call
/// metadata attached.
///
/// A `role="tool"` row reaches here only when no preceding row carries the call
/// it belongs to — a transcript whose assistant row was dropped, or one written
/// by a build that stored results without their call. Its body is then the
/// agent's own output, so it is drawn as the agent's prose surface and never as
/// a terminal block: a block is what a command the user ran is drawn as.
pub(crate) fn parse_chat_row(row: &goble_desktop_service::ChatMessage) -> ChatMessage {
    let role = match row.role.as_str() {
        "user" => ChatRole::User,
        "tool" => ChatRole::Tool,
        _ => ChatRole::Assistant,
    };
    let mut message = if role == ChatRole::Tool {
        ChatMessage::new(
            role,
            vec![ChatFragment::code_block(
                None,
                tool_result_body(&row.content),
            )],
        )
    } else {
        ChatMessage::from_markdown(role, row.content.clone())
    };
    if let Some(tc) = row.tool_calls.as_deref() {
        message = message.with_tool_calls(ToolCall::from_llm_json(tc));
    }
    message
}
