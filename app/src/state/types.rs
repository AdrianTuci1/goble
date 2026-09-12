use super::*;


/// Per-pane conversation identity + composer draft + working directory.
///
/// Each chat leaf pane owns a distinct conversation in the store plus its own
/// composer draft and cwd, so two panes never share a transcript. This is the
/// subset persisted across an app restart (`draft` + `path` + `conversation_id`
/// survive; the transcript is re-read from the store keyed by conversation id).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PaneSession {
    /// The store conversation id this pane is bound to. Empty means the pane
    /// lazily uses the currently selected conversation (`selected_id`).
    pub conversation_id: String,
    /// This pane's composer draft, independent of other panes.
    pub draft: String,
    /// This pane's working directory (shown as the composer's path label).
    pub path: String,
}

/// One worker agent execution the app has seen start and not yet seen finish,
/// built from the `agent:*` events the desktop service emits.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveExecution {
    /// The execution/trace id the service keys the run by.
    pub trace_id: String,
    /// The agent that owns the run, when the service names one.
    pub agent_id: String,
    /// The worker the run executes on.
    pub worker_id: String,
    /// The service's status for the run; `running` while it is in flight,
    /// since a finished run leaves the live set.
    pub status: String,
    /// The run's own start time (RFC3339), carried by `agent:started` from the
    /// service's ledger record. Never a time the app guessed from arrival.
    pub started_at: String,
    /// The agent's own runtime state (checklist / notes / self-feedback), last
    /// reported by `agent:state_update`.
    pub runtime_state: Option<serde_json::Value>,
    /// A short label for the most recent activity the agent reported
    /// (`agent:log` / `agent:tool_result`), so neither event is dropped.
    pub last_activity: Option<String>,
}

/// The one answer to "what work is in flight right now", read by the chrome.
///
/// Everything here is observed, not inferred: `turn_started_at` is `Some` only
/// when the app itself watched the turn start (it issued the turn); the wire
/// carries no turn-start event, so a turn whose start was not seen reports
/// `None` rather than a made-up time.
#[derive(Clone, Debug, Default)]
pub struct LiveWork {
    /// Worker agent executions currently running.
    pub executions: Vec<LiveExecution>,
    /// The active pane's tool calls that have started and not yet finished.
    pub tool_calls: Vec<ToolCall>,
    /// Whether the active pane's turn is running.
    pub turn_busy: bool,
    /// The proposal id the active pane is waiting on for approval, if any.
    pub pending_approval: Option<String>,
    /// The question the active pane is waiting on, if any.
    pub pending_question: Option<String>,
    /// When the active pane's current turn started, when the app observed it.
    pub turn_started_at: Option<std::time::Instant>,
}

impl LiveWork {
    /// Items in flight beside the active pane's turn: its tool calls plus every
    /// running execution. This is the number the topbar's live cue shows, and
    /// zero is its inert state.
    pub fn work_count(&self) -> usize {
        self.tool_calls.len() + self.executions.len()
    }

    /// The topbar spinner's phase, from the age of the observed turn start. A
    /// start that was not observed leaves the spinner on its first frame rather
    /// than inventing a time.
    pub fn spinner_phase(&self) -> f32 {
        // The same period `goble_ui`'s `TurnStatusFooter` uses, so the two
        // spinners advance together.
        const PERIOD_MS: u128 = 480;
        self.turn_started_at
            .map(|started| (started.elapsed().as_millis() % PERIOD_MS) as f32 / PERIOD_MS as f32)
            .unwrap_or(0.0)
    }
}

/// Runtime per-pane state not persisted across restarts: the transcript, the
/// suspended ask, the queued prompt and the busy flag. Re-read from the store
/// (keyed by [`PaneSession::conversation_id`]) on every refresh.
#[derive(Clone, Debug, Default)]
pub struct PaneRuntime {
    pub messages: Vec<ChatMessage>,
    /// Parsed-message cache, so a refresh re-parses only the rows whose content
    /// changed instead of every message on every event.
    pub parse_cache: MessageParseCache,
    pub pending_ask: Option<AskUserUi>,
    /// The command the harness suspended on for approval, fed by
    /// `chat:command_proposed`. While set the composer shows the proposal card.
    pub pending_command: Option<CommandProposalUi>,
    /// The app-owned selected candidate index for `pending_command`, so the
    /// composer's selection survives the per-frame rebuild.
    pub command_selection: Option<Rc<RefCell<usize>>>,
    pub queued_prompt: Option<String>,
    pub busy: bool,
    /// When this pane's current turn started, when the app observed it (it
    /// issued the turn). `None` means the start was not observed — the chrome
    /// must not invent an elapsed time for it.
    pub turn_started_at: Option<std::time::Instant>,
    /// The screen source the harness handed off to (shown inline in chat).
    pub inline_screen_source: Option<String>,
    /// A detected BYOH handoff URI from the assistant output.
    pub screen_link: Option<String>,
    /// Live tool calls still running for this pane, keyed by call id. Fed by the
    /// `chat:tool` event and overlaid on the transcript immediately, so a
    /// running call is visible before the turn ends. A finish/error clears the
    /// entry and the re-read from the store carries the terminal state.
    pub in_flight_tools: HashMap<String, ToolCall>,
    /// The model's reasoning (thinking) steps for this pane, fed by the live
    /// `chat:reasoning` events. Overlaid on the transcript as their own rows;
    /// replaced when the next reasoning turn starts (step 0).
    pub reasoning: Vec<ReasoningRow>,
    /// The sub-agent children this pane spawned, fed by the live
    /// `chat:subagent_*` events and keyed by the child's own conversation id.
    /// The parent transcript's rows read their status, activity and elapsed time
    /// from here, never from the call's arguments.
    pub sub_agents: HashMap<String, SubAgentRecord>,
}

/// One live sub-agent child, held by the pane that spawned it: the row the
/// parent's transcript draws, plus the id of the parent tool call that spawned
/// it (the key the transcript looks the record up by).
#[derive(Clone, Debug, PartialEq)]
pub struct SubAgentRecord {
    pub parent_call_id: String,
    pub row: SubAgentRow,
}

/// One pane's sub-agent child view (S6): the child's own conversation, shown in
/// the pane it was entered from. The rows are read from the store under the
/// child's chat id — the conversation S2 gave the child — and parsed through the
/// same [`MessageParseCache`] a pane's transcript uses, so the child's messages
/// reach the one message renderer, not a bespoke one.
///
/// The view borrows the agent-view mechanism (A7): entering pushed the child's
/// card into the pane's block list and pointed the pane's filter at the child's
/// conversation. `returned_to` is what the pane showed before — the shell, or
/// the parent's own agent view when the row was clicked from inside it — and
/// Esc restores it, which is how the conversation the user came from is always
/// reachable again.
#[derive(Clone, Debug)]
pub struct SubAgentChildView {
    /// The child's conversation id, which is also the store key for its rows.
    pub child_id: String,
    /// The live record's row, so the view's title can carry the child's type,
    /// description, status and elapsed time. `None` when the record is gone (the
    /// view stays open on a child that has since left the pane's live set).
    pub row: Option<SubAgentRow>,
    /// The child's transcript, in store order.
    pub messages: Vec<ChatMessage>,
    /// What the pane was showing before the child was entered.
    pub(crate) returned_to: BlockView,
    /// The child rows' parse cache, so a refresh re-parses only what changed.
    pub(crate) parse_cache: MessageParseCache,
}

impl SubAgentChildView {
    /// Re-read the child's rows from the store and take the live record's row,
    /// which is what keeps the view's title current while the child runs.
    pub(crate) fn refresh(&mut self, row: Option<SubAgentRow>, desktop: &DesktopState) {
        self.row = row;
        match desktop.list_chat_messages(&self.child_id) {
            Ok(rows) => self.messages = self.parse_cache.resolve(&rows),
            Err(e) => log::warn!("list_chat_messages({}): {e}", self.child_id),
        }
    }
}

/// One reasoning (thinking) step carried by the live `chat:reasoning` events,
/// accumulated as the step's deltas stream in.
#[derive(Clone, Debug, PartialEq)]
pub struct ReasoningRow {
    /// The store conversation the step belongs to (used to key its expand state
    /// and to scope the row to its pane).
    pub chat_id: String,
    pub step: usize,
    pub mode: String,
    pub text: String,
    pub done: bool,
}

impl PaneRuntime {
    /// Fold one live reasoning transition into the pane's accumulated steps.
    ///
    /// A `started` at step 0 begins a new reasoning turn, so the previous
    /// turn's rows are dropped; a `delta` appends to the currently open step; a
    /// `done` finalises the step with the authoritative full text.
    pub(crate) fn apply_reasoning(&mut self, event: &goble_desktop_service::ReasoningEvent) {
        use goble_desktop_service::ReasoningPhase;
        match event.phase {
            ReasoningPhase::Started => {
                let step = event.step.unwrap_or(0);
                if step == 0 {
                    self.reasoning.clear();
                }
                if let Some(row) = self.reasoning.iter_mut().find(|r| r.step == step) {
                    row.mode = event.mode.clone();
                    row.done = false;
                } else {
                    self.reasoning.push(ReasoningRow {
                        chat_id: event.chat_id.clone(),
                        step,
                        mode: event.mode.clone(),
                        text: String::new(),
                        done: false,
                    });
                }
            }
            ReasoningPhase::Delta => match self.reasoning.last_mut() {
                Some(row) => row.text.push_str(&event.delta),
                None => self.reasoning.push(ReasoningRow {
                    chat_id: event.chat_id.clone(),
                    step: 0,
                    mode: String::new(),
                    text: event.delta.clone(),
                    done: false,
                }),
            },
            ReasoningPhase::Done => {
                let step = event.step.unwrap_or(0);
                if let Some(row) = self.reasoning.iter_mut().find(|r| r.step == step) {
                    row.mode = event.mode.clone();
                    if let Some(content) = &event.content {
                        row.text = content.clone();
                    }
                    row.done = true;
                }
            }
        }
    }

    /// Open the row for a child S4 just spawned. The map is keyed by the child's
    /// own conversation id, because that is the only id the progress and finish
    /// events carry.
    pub(crate) fn apply_subagent_spawned(&mut self, event: &goble_desktop_service::SubAgentSpawnedEvent) {
        self.sub_agents.insert(
            event.subagent_id.clone(),
            SubAgentRecord {
                parent_call_id: event.parent_call_id.clone(),
                row: SubAgentRow {
                    child_id: event.subagent_id.clone(),
                    subagent_type: event.subagent_type.clone(),
                    description: event.description.clone(),
                    status: SubAgentRowStatus::Running,
                    activity: String::new(),
                    elapsed: Duration::ZERO,
                    turns: 0,
                    tool_calls: 0,
                    tokens: 0,
                    outcome: None,
                    background: event.run_in_background,
                },
            },
        );
    }

    /// Fold one live progress transition into the child's record. A record that
    /// never spawned (an app that joined mid-turn) is opened from the event, so
    /// the row still reads its status rather than falling back to the arguments.
    pub(crate) fn apply_subagent_progress(&mut self, event: &goble_desktop_service::SubAgentProgressEvent) {
        let entry = self
            .sub_agents
            .entry(event.subagent_id.clone())
            .or_insert_with(|| SubAgentRecord {
                parent_call_id: event.subagent_id.clone(),
                row: SubAgentRow {
                    child_id: event.subagent_id.clone(),
                    subagent_type: String::new(),
                    description: String::new(),
                    status: SubAgentRowStatus::Running,
                    activity: String::new(),
                    elapsed: Duration::ZERO,
                    turns: 0,
                    tool_calls: 0,
                    tokens: 0,
                    outcome: None,
                    background: false,
                },
            });
        entry.row.status = SubAgentRowStatus::from_wire(&event.status);
        if !event.activity.is_empty() {
            entry.row.activity = event.activity.clone();
        }
        entry.row.turns = event.turns;
        entry.row.tool_calls = event.tool_calls;
        entry.row.tokens = event.tokens;
        entry.row.elapsed = Duration::from_millis(event.duration_ms);
    }

    /// Fold a child's terminal status into its record: the outcome it ended with
    /// and the duration it took, replacing the live elapsed clock.
    pub(crate) fn apply_subagent_finished(&mut self, event: &goble_desktop_service::SubAgentFinishedEvent) {
        let Some(entry) = self.sub_agents.get_mut(&event.subagent_id) else {
            return;
        };
        entry.row.status = SubAgentRowStatus::from_wire(&event.status);
        entry.row.outcome = event
            .output
            .clone()
            .or_else(|| event.error.clone())
            .filter(|text| !text.is_empty());
        entry.row.activity = String::new();
        entry.row.turns = event.turns;
        entry.row.tool_calls = event.tool_calls;
        entry.row.tokens = event.tokens;
        entry.row.elapsed = Duration::from_millis(event.duration_ms);
    }
}

/// Fold one provider-reported model call into a conversation's running token
/// total. Accounting only: no part of this is a price, and a call the provider
/// reported no cache accounting for leaves the cached figure absent rather than
/// folding in a zero.
pub(crate) fn fold_token_usage(
    total: &mut goble_ui::TokenUsage,
    event: &goble_desktop_service::TokenUsageEvent,
) {
    total.input = total.input.saturating_add(event.input);
    total.output = total.output.saturating_add(event.output);
    if let Some(cached) = event.cached {
        total.cached = Some(total.cached.unwrap_or(0).saturating_add(cached));
    }
}

/// One `chat:usage` event as the app receives it: which conversation spent the
/// tokens, and what the model call reported.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct TokenUsagePayload {
    pub chat_id: String,
    pub usage: goble_desktop_service::TokenUsageEvent,
}

/// The rich-input controls of one pane.
///
/// The workspace is only a container: everything the composer shows for one
/// pty/agent session (auto-approve, model, git branch, harness mode and the
/// dropdown open flags) lives here, keyed by pane id, so two pty/agent windows
/// in the same workspace keep independent controls instead of sharing the
/// window-global ones.
#[derive(Clone, Debug)]
pub struct PaneControls {
    /// Whether this pane auto-approves `ask_user` questions.
    pub auto_approve: bool,
    /// This pane's model (shown as the composer's model label).
    pub model: String,
    /// This pane's git branch pill value.
    pub branch: String,
    /// Whether the harness is active for this pane: the rich input routes
    /// turns to the agent instead of the plain shell. Toggled at the rich
    /// input with Cmd+Enter; Esc returns the pane to the plain pty.
    pub harness_mode: bool,
    /// Which view this pane's terminal surface shows: the shell's own history
    /// (the terminal filter) or one conversation's agent view. Cmd+Enter and a
    /// click on a conversation's card enter that view; Esc returns to the
    /// terminal.
    pub view: BlockView,
    pub model_menu_open: Rc<RefCell<bool>>,
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_menu_open: Rc<RefCell<bool>>,
    pub branch_menu_open: Rc<RefCell<bool>>,
    /// Where this pane's rich input draws its insertion beam, as a character
    /// index into the draft. App-owned because the composer element is rebuilt
    /// every frame and would otherwise lose the position on each one.
    pub caret: Rc<RefCell<usize>>,
    /// This pane's modal (vim) editing state: the mode, the half-typed command
    /// and the registers. One per pane, so two editors keep independent modes;
    /// the composer is rebuilt every frame, so it lives here and not in the
    /// element tree.
    pub vim: Rc<RefCell<VimState>>,
}

impl PaneControls {
    pub fn new(model: String, auto_approve: bool, branch: String) -> Self {
        Self {
            auto_approve,
            model,
            branch,
            harness_mode: false,
            view: BlockView::Terminal,
            model_menu_open: Rc::new(RefCell::new(false)),
            harness_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_open: Rc::new(RefCell::new(false)),
            branch_menu_open: Rc::new(RefCell::new(false)),
            caret: Rc::new(RefCell::new(0)),
            vim: Rc::new(RefCell::new(VimState::new())),
        }
    }
}

impl Default for PaneControls {
    fn default() -> Self {
        Self::new(String::new(), false, String::new())
    }
}
