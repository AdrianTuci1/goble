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

/// Build a terminal-style block from a stored tool-result message. The harness
/// writes tool output as `"<call_id>\n<output>"`, so the first line names the
/// call and the remaining lines are the result body.
///
/// A command's result is drawn as the same block the tool-call path and the
/// pane build: `command` is the `command` argument of the call the row names,
/// resolved from the assistant row's persisted `tool_calls` column, and the
/// body goes through [`TerminalData::for_command`], so the command line is
/// highlighted as shell and the output keeps its ANSI colours. Every other
/// tool result has no command line, and one is not invented for it: the body
/// keeps the plain output rendering.
///
/// The status is passed in, read from the call the row names: the persisted
/// `tool_calls` column carries a `ToolCallStatus`, so nothing here infers
/// success or failure from the result text.
pub(crate) fn tool_terminal_data(
    content: &str,
    status: TerminalStatus,
    command: Option<&str>,
) -> TerminalData {
    let mut parts = content.splitn(2, '\n');
    let title = parts.next().unwrap_or("tool").trim();
    let body = parts.next().unwrap_or("");

    match command.filter(|command| !command.is_empty()) {
        Some(command) => TerminalData::for_command(command, body, status),
        None => tool_body_block(title, body.trim(), status),
    }
}

/// The plain block a tool result gets when it has no command line to draw: the
/// call id as the title and the body as mono output lines.
pub(crate) fn tool_body_block(title: &str, body: &str, status: TerminalStatus) -> TerminalData {
    let mut lines = Vec::new();
    for line in body.lines() {
        let text = line.trim_end().to_string();
        if text.is_empty() {
            lines.push(TerminalLine::info(" "));
        } else {
            lines.push(TerminalLine::output(text));
        }
    }
    if lines.is_empty() {
        lines.push(TerminalLine::info("(no output)"));
    }

    TerminalData::new(
        if title.is_empty() { "tool" } else { title }.to_string(),
        lines,
    )
    .with_status(status)
}

/// The command a persisted call carries, when its tool parses into the command
/// family. It reads the same parse the renderer does and the same `command`
/// argument, so the persisted block is the one the tool-call path builds. A
/// call that is not a command, or whose arguments carry no command, has none.
pub(crate) fn command_of(call: &ToolCall) -> Option<String> {
    if tool_kind_for(&call.name) != ToolKind::Execute {
        return None;
    }
    let arguments: serde_json::Value = serde_json::from_str(&call.arguments).ok()?;
    arguments.get("command")?.as_str().map(str::to_string)
}

/// The live turn-status footer's state for one pane, read from the pane's own
/// runtime plus the workspace's running executions (C1's live data).
///
/// The busy activity is resolved from what is genuinely under way: a pending
/// approval, a pending question, an in-flight tool call, or — with none — the
/// model's phase, read from whether its reasoning step is still open. `elapsed`
/// is the age of the observed turn start; a turn whose start was not observed
/// reports `None` rather than an invented time.
pub(crate) fn pane_turn_status(rt: Option<&PaneRuntime>, executions: usize) -> TurnStatus {
    let Some(rt) = rt else {
        // A pane with no runtime yet has no turn of its own; the workspace's
        // running executions are still in flight.
        return still_running_status(0, executions);
    };
    let in_flight = rt.in_flight_tools.len();
    if rt.busy {
        let activity = if rt.pending_command.is_some() {
            TurnActivity::WaitingOnApproval
        } else if rt.pending_ask.is_some() {
            TurnActivity::WaitingOnQuestion
        } else if let Some(call) = rt.in_flight_tools.values().min_by(|a, b| a.id.cmp(&b.id)) {
            TurnActivity::Tool {
                name: call.name.clone(),
                command: command_of(call),
            }
        } else if rt.reasoning.last().map(|row| !row.done).unwrap_or(true) {
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

    still_running_status(in_flight, executions)
}

/// The idle-with-work state: one entry per kind that is really running, or
/// [`TurnStatus::Idle`] (zero height) when nothing is.
pub(crate) fn still_running_status(in_flight: usize, executions: usize) -> TurnStatus {
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
    if kinds.is_empty() {
        TurnStatus::Idle
    } else {
        TurnStatus::StillRunning { kinds }
    }
}

/// The status a tool call's persisted lifecycle maps to on a terminal block: a
/// call that has not reached a terminal state is a block still running.
pub(crate) fn tool_call_terminal_status(status: ToolCallStatus) -> TerminalStatus {
    match status {
        ToolCallStatus::Pending | ToolCallStatus::Running => TerminalStatus::Running,
        ToolCallStatus::Finished => TerminalStatus::Success,
        ToolCallStatus::Error => TerminalStatus::Error,
    }
}

/// The call id a `role="tool"` row belongs to: the harness writes the result
/// row as `"<call_id>\n<output>"`, so the first line names the call whose
/// persisted status is the result's outcome.
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
    /// The status resolved for this row at parse time. A tool-result row draws
    /// its call's status, which lives on a different row, so an entry is only
    /// reused while that resolved status is unchanged too.
    status: Option<TerminalStatus>,
    /// The command resolved for this row at parse time. It is the same kind of
    /// cross-row value as `status`: a result row's command line changes when
    /// its call's command does, though the row's own text did not.
    command: Option<String>,
    message: ChatMessage,
}

impl CachedMessage {
    /// Whether `row` still carries the fields this entry was parsed from. An
    /// edit to any of them (a streaming delta appends to `content`) invalidates
    /// the entry so the message is re-parsed.
    pub(crate) fn matches(
        &self,
        row: &goble_desktop_service::ChatMessage,
        status: Option<TerminalStatus>,
        command: Option<&str>,
    ) -> bool {
        self.role == row.role
            && self.content == row.content
            && self.tool_calls == row.tool_calls
            && self.status == status
            && self.command.as_deref() == command
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
    pub fn resolve(&mut self, rows: &[goble_desktop_service::ChatMessage]) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(rows.len());
        let mut next: HashMap<String, CachedMessage> = HashMap::with_capacity(rows.len());
        // The calls seen so far this pass, by id. The assistant row that
        // persisted a call precedes its result row, so a tool-result row can
        // read its call's status and command here without re-parsing the
        // tool-call JSON.
        let mut statuses: HashMap<String, ToolCallStatus> = HashMap::new();
        let mut commands: HashMap<String, String> = HashMap::new();
        for row in rows {
            let call_id = (row.role == "tool").then(|| tool_result_call_id(&row.content));
            let status = call_id
                .and_then(|id| statuses.get(id).copied())
                .map(tool_call_terminal_status);
            let command = call_id.and_then(|id| commands.get(id).cloned());
            let message = match self.entries.get(&row.id) {
                Some(cached) if cached.matches(row, status, command.as_deref()) => {
                    cached.message.clone()
                }
                _ => {
                    self.parses += 1;
                    parse_chat_row(row, status, command.as_deref())
                }
            };
            for call in &message.tool_calls {
                statuses.insert(call.id.clone(), call.status);
                if let Some(command) = command_of(call) {
                    commands.insert(call.id.clone(), command);
                }
            }
            next.insert(
                row.id.clone(),
                CachedMessage {
                    role: row.role.clone(),
                    content: row.content.clone(),
                    tool_calls: row.tool_calls.clone(),
                    status,
                    command,
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
            *existing = call.clone();
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

/// Parse one stored row into a [`ChatMessage`]: a tool result becomes a terminal
/// block, everything else is Markdown, and tool-call metadata is attached.
/// `status` and `command` are the values resolved for the call a tool-result row
/// belongs to, read from the persisted `tool_calls` column.
pub(crate) fn parse_chat_row(
    row: &goble_desktop_service::ChatMessage,
    status: Option<TerminalStatus>,
    command: Option<&str>,
) -> ChatMessage {
    let role = match row.role.as_str() {
        "user" => ChatRole::User,
        "tool" => ChatRole::Tool,
        _ => ChatRole::Assistant,
    };
    // Tool results are stored as "<call_id>\n<output>". Present them as a
    // distinct terminal block instead of assistant prose so the user can tell
    // execution output apart.
    let mut message = if role == ChatRole::Tool {
        ChatMessage::new(
            role,
            vec![ChatFragment::terminal(tool_terminal_data(
                &row.content,
                status.unwrap_or(TerminalStatus::Idle),
                command,
            ))],
        )
    } else {
        ChatMessage::from_markdown(role, row.content.clone())
    };
    if let Some(tc) = row.tool_calls.as_deref() {
        message = message.with_tool_calls(ToolCall::from_llm_json(tc));
    }
    message
}
