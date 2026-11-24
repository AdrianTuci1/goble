use std::cell::RefCell;
use std::rc::Rc;
use std::time::SystemTime;

use goble_terminal::blocks::BlockView;
use goble_terminal::BlockOwner;
use goble_ui::elements::{
    AppContext, ComposerModelTray, CrossAxisAlignment, Element, Expanded, Flex, LaneEdge,
    MainAxisSize, ModelTrayHarness, ModelTrayModel, PopupMenuItem,
};
use goble_ui::{ChatFragment, ChatMessage, ChatRole, ChatView, WorkSection};

use crate::emulator::VisibleBlock;
use crate::state::PaneControls;

use super::super::{UiActions, UiSnapshot};
use super::{
    build_agent_error, build_agent_header, build_sub_agent_child_view, build_terminal_dock,
    chat_action_relay, pane_session_snapshot, PaneChatSnapshot, PaneHeading,
};

/// If `pane_id` has a live PTY session with output, draw its newest command as
/// an inline terminal block in the transcript so a terminal command (`!cmd`)
/// shows its output (warp-new style executed-command block). The block sits
/// where its command started, ordered with the chat rows by time. It is
/// recomputed each frame from the session buffer, so a long-running command
/// streams as it runs without ever growing the stored transcript.
fn with_inline_terminal(
    messages: Vec<ChatMessage>,
    terminal: &std::rc::Rc<std::cell::RefCell<crate::terminal::TerminalRegistry>>,
    pane_id: u64,
) -> Vec<ChatMessage> {
    // The pane's own executed-command block: the transcript draws it with the
    // same terminal block the pane does, so a command the user ran is one block
    // wherever it appears. A command the agent ran is not this: its call is part
    // of the agent's reply and is drawn continuously under the row that names it.
    //
    // A conversation reads its own shell, never the pane's terminal: a `!` line
    // typed in the agent view runs beside the terminal, so the terminal's own
    // commands stay out of the transcript and this one stays out of the shell
    // behind Esc.
    let block = {
        let reg = terminal.borrow();
        reg.agent_session(pane_id)
            .and_then(crate::ui::terminal::executed_command_block_at)
    };
    let Some((data, started)) = block else {
        return messages;
    };
    // Where the command began, not where the transcript ends: a `!` line typed
    // between two replies stays between them.
    merge_conversation_commands(
        messages,
        vec![(
            started,
            ChatMessage::new(ChatRole::Tool, vec![ChatFragment::terminal(data)]),
        )],
    )
}

/// The transcript rows for the commands typed *inside* a conversation — the `!`
/// lines the user ran from the pane's agent input — so the conversation draws
/// them where they ran instead of leaving them behind the shell's own view
/// (Esc). Each is the shared terminal block: a command the user typed is the
/// one thing that is drawn segmented, and it is drawn the same in a
/// conversation as in the pane.
///
/// A block the agent owns is left out: its turn already carries that command as
/// its own tool-call row, drawn continuously in the reply rather than as a
/// block. The caller passes the conversation's own blocks, so a shell block is
/// never among them: the shell's history keeps its blocks to itself, and a
/// conversation opened from the terminal still starts on a clean sheet.
fn conversation_command_rows(blocks: &[VisibleBlock]) -> Vec<(Option<SystemTime>, ChatMessage)> {
    blocks
        .iter()
        .filter(|block| block.owner == BlockOwner::User)
        .filter_map(|block| {
            crate::ui::terminal::section_data(block).map(|data| {
                (
                    block.started_at,
                    ChatMessage::new(ChatRole::Tool, vec![ChatFragment::terminal(data)]),
                )
            })
        })
        .collect()
}

/// Append the commands typed inside `conversation_id` to the transcript. See
/// [`conversation_command_rows`].
///
/// A command is placed where it started, not at the end: each row carries the
/// time its block began, each chat row the time it was written, so a `!` line
/// typed mid-turn stays between the reply it followed and the reply that came
/// after it. A row with no time to compare (the live reasoning block, an
/// in-flight call) keeps its place, and a command with no start time stays at
/// the end — the order is only as good as the clocks the two sides carry.
fn with_conversation_commands(
    messages: Vec<ChatMessage>,
    state: &UiSnapshot,
    pane_id: u64,
    conversation_id: &str,
) -> Vec<ChatMessage> {
    if conversation_id.is_empty() {
        return messages;
    }
    let view = BlockView::Agent {
        conversation_id: conversation_id.to_string(),
    };
    // The conversation's commands live in its own shell; reading the pane's
    // terminal here is what made a `!` command appear on both surfaces.
    let blocks = state.terminal.borrow().agent_visible_blocks(pane_id, &view);
    merge_conversation_commands(messages, conversation_command_rows(&blocks))
}

/// Fold the conversation's commands into its transcript in the order they
/// began. A command is due before the first chat row written after it started,
/// and a row with no time keeps its place; whatever is left is appended.
fn merge_conversation_commands(
    messages: Vec<ChatMessage>,
    commands: Vec<(Option<SystemTime>, ChatMessage)>,
) -> Vec<ChatMessage> {
    if commands.is_empty() {
        return messages;
    }
    let mut merged = Vec::with_capacity(messages.len() + commands.len());
    let mut commands = commands.into_iter().peekable();
    for message in messages {
        if let Some(at) = message_start(&message) {
            while commands
                .peek()
                .is_some_and(|(start, _)| start.is_some_and(|start| start < at))
            {
                merged.push(commands.next().expect("peeked").1);
            }
        }
        merged.push(message);
    }
    merged.extend(commands.map(|(_, row)| row));
    merged
}

/// When a chat row was written, from the store's RFC3339 `created_at` that
/// [`parse_chat_row`](crate::state::parse_chat_row) keeps on the message.
fn message_start(message: &ChatMessage) -> Option<SystemTime> {
    let raw = message.timestamp.as_deref()?;
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    Some(parsed.with_timezone(&chrono::Utc).into())
}

/// The notice a pane leads its transcript with.
///
/// A viewer pane draws its worker connection's state there: the pane has no
/// shell to show, so the state of the session is the one thing it must report
/// (connecting, connected, and — if the connection drops — that it dropped).
/// Every other pane draws the missing-model-key notice it had before, from the
/// window-level banner flag.
fn notice_for(
    session: &PaneChatSnapshot,
    state: &UiSnapshot,
    app: &AppContext,
    actions: &UiActions,
) -> Option<Box<dyn Element>> {
    match &session.worker {
        Some(worker) => Some(crate::ui::worker::build_worker_notice(
            app, state, actions, worker,
        )),
        None => state
            .show_llm_key_banner
            .then(|| build_agent_error(app, actions, state.llm_notice_heading)),
    }
}

/// What the composer's directory control reads for this pane.
///
/// A viewer pane has no local working directory — its conversation runs on a
/// worker — so it names where the work actually runs instead of drawing this
/// machine's path under a conversation that is not on it.
fn composer_path(session: &PaneChatSnapshot) -> String {
    match &session.worker {
        Some(worker) => worker.location_label(),
        None => crate::state::display_path(&session.composer_path),
    }
}

/// Run a composer gesture after focusing the pane its control belongs to.
///
/// The app's composer actions resolve the pane through `active_pane_id` when
/// they run, while the composer knows which pane it was built for. A gesture
/// can therefore fire after the active pane has already moved — a press on
/// another pane's body activates it on mouse-down, and the key events that
/// follow reach the still-built composer before the next frame unfocuses it —
/// and act on the wrong conversation. These wrappers make the gesture name its
/// own pane first, the same order the terminal bar already uses for its send
/// and Cmd+Enter.
fn in_pane<F>(
    activate: &Rc<RefCell<dyn FnMut(u64)>>,
    pane_id: u64,
    mut action: F,
) -> impl FnMut() + 'static
where
    F: FnMut() + 'static,
{
    let activate = Rc::clone(activate);
    move || {
        (activate.borrow_mut())(pane_id);
        action();
    }
}

/// [`in_pane`] for a gesture that carries an argument (the draft text, the row
/// index).
fn in_pane_with<A, F>(
    activate: &Rc<RefCell<dyn FnMut(u64)>>,
    pane_id: u64,
    mut action: F,
) -> impl FnMut(A) + 'static
where
    F: FnMut(A) + 'static,
{
    let activate = Rc::clone(activate);
    move |arg| {
        (activate.borrow_mut())(pane_id);
        action(arg);
    }
}

/// Agent chat tab: a header row with the agent identity/status/copy/restart,
/// then the message transcript + composer (which fills the remaining space).
///
/// `pane_id` selects this pane's own transcript + composer draft + path, so
/// every chat pane renders an independent session instead of the global one.
///
/// `harness_open` says this agent view is drawn over a shell the pane owns —
/// the terminal pane's harness view — so its transcript carries the
/// conversation's own terminal blocks rather than the pane's inline ones.
///
/// `on_escape` is the transcript's Escape hook. The chat pane passes `None`:
/// its terminal is the bottom dock toggled from the header, never a second
/// surface reached with Escape. A legacy terminal pane still passes its return
/// to the shell.
pub fn build_agent_chat(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    active: bool,
    harness_open: bool,
    on_escape: Option<Rc<RefCell<dyn FnMut()>>>,
) -> Box<dyn Element> {
    let session = pane_session_snapshot(state, pane_id);
    // The host this pane's shell is on, when a submitted `ssh` line bound it
    // (S1). The agent view is the same pane's other surface, so it names the
    // same session over its own input; a local shell has no entry and draws no
    // chip.
    let ssh_session = state.terminal.borrow().ssh_session(pane_id).cloned();

    // A pane showing a sub-agent's child view (S6) shows that child's own
    // conversation, not its own: the child's transcript under a title naming the
    // child, and no composer of the parent's.
    if let Some(child) = session.sub_agent_view.clone() {
        return build_sub_agent_child_view(app, state, actions, pane_id, active, &session, &child);
    }

    // The bar names the conversation this pane talks to, or the placeholder
    // while it has no subject yet: a pane on a worker starts as a cloud agent,
    // so its placeholder says that instead of the native agent's label.
    let heading = state.pane_titles.get(&pane_id).cloned().unwrap_or_else(|| {
        if session.worker.is_some() {
            crate::state::NEW_CLOUD_AGENT_TITLE.to_string()
        } else {
            crate::state::NEW_AGENT_TAB_LABEL.to_string()
        }
    });
    // The bar names the harness this surface works with: the CLI agent in the
    // pane's pty when one runs (the chip asks the registry first), else the
    // conversation's own harness, else the native one this view runs.
    let surface_harness = state
        .terminal
        .borrow()
        .pane_harness(pane_id)
        .or_else(|| {
            state
                .conversations
                .iter()
                .find(|entry| entry.id == session.conversation_id)
                .and_then(|entry| goble_core::harness::harness_for(&entry.harness))
        })
        .or_else(|| goble_core::harness::harness_for("goble"));
    // A CLI harness owns its own model and approval policy. The app's model
    // switcher and always-approve control are the native agent's, so a pane
    // talking to a CLI harness draws neither; the harness chip names what
    // answers instead.
    let native_harness = surface_harness.is_none_or(|spec| spec.id == "goble");
    // This pane's committed content-lane width, if the user dragged one. The
    // lane only meters this pane's conversation column (drawn on the darker
    // `Bg`); the topbar and the work dock stay full-pane-width chrome bands and
    // never follow the adjustable width.
    // 309: the transcript/text lane is narrower than the composer band, so a
    // full-width line can never run into the composer controls.
    let content_lane_pref = state
        .content_lane_prefs
        .get(&pane_id)
        .copied()
        .or(Some(820.0));
    // The window's single chrome row owns the title and actions now; the panel
    // starts directly under it with no second header row.
    let header: Box<dyn Element> = goble_ui::elements::Empty::new()
        .with_size(goble_ui::geometry::vec2f(0.0, 0.0))
        .finish();
    let _ = (&on_escape, surface_harness, heading);

    // The rich-input controls belong to this pane, not to the workspace: two
    // pty/agent panes side by side keep their own model, branch and dropdown
    // open flags.
    let controls = state
        .pane_controls
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| {
            PaneControls::new(
                state.selected_model.clone(),
                state.approval_mode,
                String::new(),
            )
        });

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_pane_activate = actions.on_pane_activate.clone();
    // The focus handler is a `move` closure of its own; the shared handle above
    // stays available for the composer's per-pane gesture wrappers.
    let on_activate_for_focus = on_pane_activate.clone();
    let on_diff_panel = actions.on_diff_panel.clone();
    let on_send_message = actions.on_send_message.clone();
    let on_attach = actions.on_attach.clone();
    let on_voice = actions.on_voice.clone();
    let on_select_model = actions.on_select_model.clone();
    let on_select_approval_mode = actions.on_select_approval_mode.clone();
    let on_stop = actions.on_stop.clone();
    let on_answer_ask = actions.on_answer_ask.clone();
    let on_skip_ask = actions.on_skip_ask.clone();
    let on_command_decision = actions.on_command_decision.clone();
    let on_send_queued = actions.on_send_queued.clone();
    let on_dismiss_queued = actions.on_dismiss_queued.clone();
    let on_open_url = actions.on_open_url.clone();
    // The row is drawn in this pane, so the child view (S6) opens in this pane:
    // the pane id is baked into the handler here rather than carried by the
    // row's action, which names only the child's conversation.
    let on_open_sub_agent: Rc<RefCell<dyn FnMut(String)>> = {
        let open = actions.on_open_sub_agent.clone();
        Rc::new(RefCell::new(move |child_id: String| {
            (open.borrow_mut())(pane_id, child_id)
        }))
    };
    // The Stop is drawn in this pane's transcript, so the pane id is baked in
    // here too: the row names only the child it wants stopped.
    let on_stop_sub_agent: Rc<RefCell<dyn FnMut(String)>> = {
        let stop = actions.on_stop_sub_agent.clone();
        Rc::new(RefCell::new(move |child_id: String| {
            (stop.borrow_mut())(pane_id, child_id)
        }))
    };
    // No tasks/sub-agents strip in the chat panel header: work items live in
    // the side panel's rail/Overview.
    let work_dock: Option<Box<dyn Element>> = None;
    let work_dock: Option<Box<dyn Element>> = None;
    let on_select_harness = actions.on_select_harness.clone();
    let on_select_dir = actions.on_select_dir.clone();
    let on_select_branch = actions.on_select_branch.clone();
    let on_slash_move = actions.on_slash_move.clone();
    let on_slash_close = actions.on_slash_close.clone();
    let on_slash_dismiss = actions.on_slash_dismiss.clone();
    let on_cmd_enter = actions.on_cmd_enter.clone();
    let on_send_to_cloud = actions.on_send_to_cloud.clone();

    // Model dropdown: one item per available model; the current one is marked
    // selected. Selecting an item maps the index back to a model name.
    let model_items = state
        .models
        .iter()
        .map(|name| {
            let mut item = PopupMenuItem::new(name.clone()).with_icon("cpu");
            if name == &controls.model {
                item = item.selected();
            }
            item
        })
        .collect::<Vec<_>>();
    let on_model_select = actions.on_model_select.clone();
    let models_for_select = state.models.clone();
    let model_menu_open = controls.model_menu_open.clone();
    let model_index = controls.model_index.clone();

    // The tray the same control opens: the models a pane can run, grouped under
    // the harness that owns each one. The groups are recognition — a mark and a
    // name over the rows that harness runs — and a pick is always a model. The
    // rows are the desktop's catalog, so the tray lists what a turn can run.
    let tray_harnesses: Vec<ModelTrayHarness> = state
        .model_groups
        .iter()
        .map(|group| {
            ModelTrayHarness::new(group.harness.clone(), group.label.clone()).with_models(
                group
                    .models
                    .iter()
                    .map(|model| {
                        ModelTrayModel::new(model.id.clone(), model.name.clone())
                            .with_thinking(model.thinking.clone())
                    })
                    .collect(),
            )
        })
        .collect();
    // What a pick resolves to: the model ids in the order the tray draws them,
    // since the tray reports the row it took rather than the id behind it.
    let tray_model_ids: Vec<String> = state
        .model_groups
        .iter()
        .flat_map(|group| group.models.iter().map(|model| model.id.clone()))
        .collect();

    // Append the live terminal blocks this pane's conversation owns, so a
    // command run from the agent input (`!cmd`) shows where it ran.
    //
    // The terminal pane's harness view draws the conversation's own commands —
    // the `!` lines typed in it, which belong to the conversation alone — and
    // none of the shell's history: that view is opened *from* the shell, and
    // carrying the shell's blocks into it would open a new chat with the
    // terminal's screen pasted in. A plain chat pane has no shell view to fall
    // back on, so it shows the pane's newest command block instead.
    let messages = if harness_open {
        with_conversation_commands(
            session.messages.clone(),
            state,
            pane_id,
            &session.conversation_id,
        )
    } else {
        with_inline_terminal(session.messages.clone(), &state.terminal, pane_id)
    };
    let mut chat = ChatView::new()
        .with_header(header)
        // A viewer pane leads its transcript with the worker connection's own
        // state — what the worker streamed is the whole of what this pane has,
        // and a dropped connection has to be readable where the conversation is.
        // Every other pane leads with the missing-model-key notice, if it has
        // one.
        .with_notice(notice_for(&session, state, app, actions))
        // A failed turn is reported in the conversation it failed in, under
        // the last message, as well as in the inbox.
        .with_turn_error(session.turn_error.clone())
        .with_messages(messages)
        // The transcript scrolls and follows the stream; the state is
        // app-owned per pane so a chosen scrollback position is held.
        .with_scroll_state(session.scroll.clone())
        // Drag-to-copy over the transcript: the pane's own selection, so the
        // highlight follows the text the user pressed on and is copied on
        // release.
        .with_transcript_selection(session.text_selection.clone())
        // Per-block terminal filter state is app-owned (shared) so the filter
        // tray's open flag + selection survive the per-frame rebuild; the copy
        // handler copies a terminal block's text to the clipboard.
        .with_terminal_filters(state.terminal_filters.clone())
        // The app-owned reasoning expand map: a thinking row the user opened
        // survives the per-frame rebuild.
        .with_reasoning_expanded(state.reasoning_expanded.clone())
        // The app-owned tool-call fold map: a folded call survives the rebuild.
        .with_tool_fold(state.tool_fold.clone())
        .with_tool_hover(state.tool_hover.clone())
        .with_changed_files_open(session.changed_files_open.clone())
        // The changed-files card's Review opens the SIDE PANEL's Review tab,
        // not a pane beside the conversation; a hidden panel is expanded first.
        .with_on_review_changes({
            let side = actions.on_side_tab.clone();
            let toggle = actions.on_toggle_right_sidebar.clone();
            let panel_open = state.right_sidebar_open;
            move || {
                (side.borrow_mut())(crate::state::SideTabAction::Add(
                    crate::state::SideTabKind::Review,
                ));
                if !panel_open {
                    (toggle.borrow_mut())();
                }
            }
        })
        // This pane's own filter bar state (seeded per pane before the
        // snapshot), so opening it in one pane leaves the siblings alone.
        .with_global_terminal_filter(state.terminal_global_filters.get(&pane_id).cloned())
        .with_on_copy_terminal(Some(actions.on_copy_terminal.clone()))
        // The live sub-agent records for this pane: the transcript's rows read
        // their status, activity and elapsed time from these, not from the
        // spawn call's arguments.
        .with_sub_agents(session.sub_agents.clone())
        .with_sub_agent_model(controls.model.clone())
        .with_spinner_phase(session.work_dock_phase)
        // The pane's work dock: drawn under the header, above the transcript.
        .with_work_dock(work_dock)
        // A link inside a paragraph dispatches `ChatAction::OpenUrl`; a
        // sub-agent card's controls enter and stop the child this pane spawned.
        .with_on_action(chat_action_relay(
            on_open_url,
            actions.on_copy_terminal.clone(),
            actions.on_explorer_file_click.clone(),
            Some(on_open_sub_agent.clone()),
            Some(on_stop_sub_agent.clone()),
        ))
        .with_composer_value(session.composer_draft.clone())
        // Only the active pane's composer is focused, so a background chat pane
        // never swallows keys before the active pane (e.g. a terminal) sees them.
        .with_composer_focused(active && session.composer_focused)
        .with_composer_caret(controls.caret.clone())
        .with_composer_anchor(controls.selection_anchor.clone())
        // The clipboard is the pane's own: a copy, a cut and a paste in the
        // rich input read it, and the vim registers share the same handle.
        .with_composer_clipboard(actions.clipboard.clone())
        // The transcript's own fold key (`e`) is the active pane's too.
        .with_pane_active(active)
        .with_composer_path(composer_path(&session))
        .with_composer_ssh_session(ssh_session)
        .with_composer_attachments(session.composer_attachments.clone())
        .with_composer_stop_visible(session.agent_busy)
        // The live turn-status footer, fed by C1's real live state per pane. It
        // is information only, so it carries no click handler.
        .with_turn_status(session.turn_status.clone())
        .with_composer_on_change(move |text| (on_composer_change.borrow_mut())(text))
        // Focusing a pane's composer also activates that pane, so the composer
        // draft / send route to the pane the user is actually typing in.
        .with_composer_on_focus_change(move |focused| {
            if focused {
                (on_activate_for_focus.borrow_mut())(pane_id);
            }
            (on_composer_focus.borrow_mut())(focused);
        })
        .with_composer_on_attach(in_pane(&on_pane_activate, pane_id, move || {
            (on_attach.borrow_mut())()
        }))
        .with_composer_on_voice(move || (on_voice.borrow_mut())())
        .with_composer_on_select_model(move || (on_select_model.borrow_mut())())
        .with_composer_on_stop(in_pane(&on_pane_activate, pane_id, move || {
            (on_stop.borrow_mut())()
        }))
        .with_pending_ask(session.pending_ask.clone())
        .with_on_answer_ask(move |resp, cred| (on_answer_ask.borrow_mut())(resp, cred))
        .with_on_skip_ask(move || (on_skip_ask.borrow_mut())())
        .with_command_proposal(session.pending_command.clone())
        .with_command_proposal_selection(session.command_selection.clone())
        .with_command_proposal_scroll(session.command_scroll.clone())
        .with_command_proposal_draft(
            session.command_draft.clone(),
            session.command_caret.clone(),
            session.command_editor_scroll.clone(),
        )
        .with_on_select_approval_mode(move |mode| {
            (on_select_approval_mode.borrow_mut())(pane_id, mode)
        })
        .with_on_command_decision(move |id, decision| {
            (on_command_decision.borrow_mut())(pane_id, id, decision)
        })
        .with_queued_prompt(session.queued_prompt.clone())
        .with_on_send_queued(in_pane(&on_pane_activate, pane_id, move || {
            (on_send_queued.borrow_mut())()
        }))
        .with_on_dismiss_queued(in_pane(&on_pane_activate, pane_id, move || {
            (on_dismiss_queued.borrow_mut())()
        }))
        .with_composer_model_menu(model_items, model_menu_open, model_index, move |index| {
            if let Some(name) = models_for_select.get(index) {
                (on_model_select.borrow_mut())(pane_id, name.clone());
            }
        })
        // The submit paths all resolve the pane through the active-pane
        // contract, so the pane whose composer was pressed becomes the active
        // one first: a click on a background pane's send sends *its* draft to
        // *its* conversation.
        .with_on_send(in_pane_with(&on_pane_activate, pane_id, move |text| {
            (on_send_message.borrow_mut())(text)
        }))
        .with_composer_on_cmd_enter(in_pane_with(&on_pane_activate, pane_id, move |text| {
            (on_cmd_enter.borrow_mut())(text)
        }))
        // `⌘⌥↵`: the same submit, routed to the cloud medium.
        .with_composer_on_send_to_cloud(in_pane_with(&on_pane_activate, pane_id, move |text| {
            (on_send_to_cloud.borrow_mut())(text)
        }))
        // The attach (`+`) card is app-owned like the model card: its open flag
        // and highlighted row live on the pane's controls so the menu survives
        // the per-frame rebuild.
        .with_composer_attach_menu(
            controls.attach_menu_open.clone(),
            controls.attach_menu_highlight.clone(),
        );

    // The pane's content lane: the transcript, hint strip and rich input are
    // centred inside THIS pane's width by the app's width axis, while the
    // pane's own header above them keeps the full pane width and never
    // resizes with the content. The committed width is the pane's own (a
    // sibling pane keeps its width/ratio), and only the pane a drag started
    // in carries the drag state and its accent handle line. The lane element
    // reports element-level gestures, so the pane id is baked into the start
    // callback here: the host writes that pane's entry.
    let on_content_lane_start = {
        let start = actions.on_content_lane_drag_start.clone();
        Rc::new(RefCell::new(
            move |pointer_x: f32, edge: LaneEdge, base_width: f32, column: f32| {
                (start.borrow_mut())(pane_id, pointer_x, edge, base_width, column)
            },
        ))
    };
    let pane_dragging = state.content_lane_drag_pane == Some(pane_id);
    chat = chat.with_content_lane(
        state.content_lane_prefs.get(&pane_id).copied(),
        pane_dragging,
        pane_dragging
            .then_some(state.content_lane_drag_edge)
            .flatten(),
        on_content_lane_start,
        actions.on_content_lane_drag_move.clone(),
        actions.on_content_lane_drag_end.clone(),
    );

    // A viewer pane that has not run yet is a cloud agent waiting for its
    // first prompt. The surface names what it starts — the cloud over the
    // landing, and an input that invites the run — the way the reference
    // tool's "Kick off a cloud agent" page does. Once the conversation has a
    // message the landing is gone and the transcript is the surface.
    if session.worker.is_some() && session.messages.is_empty() {
        chat = chat
            .with_empty_state(
                crate::state::NEW_CLOUD_AGENT_TITLE,
                "It runs on the worker, so the work keeps going after you close the laptop.",
            )
            .with_empty_state_icon("cloud")
            .with_composer_placeholder("Kick off a cloud agent");
    }

    // The model control is the client's own choice, so it is drawn only where
    // the client's choice is the one the turn runs on. A viewer pane's
    // conversation runs on a worker, which resolves its own model
    // (`LLM_MODEL`), and the remote turn carries none (`build_remote_turn`), so
    // the pane draws no control whose selection the route would drop: the
    // environment it can honestly choose is its only submit-time control (A3,
    // model half).
    if session.worker.is_none() && native_harness {
        chat = chat.with_composer_model_label(controls.model.clone());
        // The same control opens the tray: the model chip is the one entry
        // point, and the band above the input is where the columns show.
        {
            let on_model_select = actions.on_model_select.clone();
            let on_thinking_select = actions.on_thinking_select.clone();
            let tray = ComposerModelTray::new(
                tray_harnesses.clone(),
                controls.model_tray_state.clone(),
                controls.model_tray_open.clone(),
            )
            .with_current(controls.model.clone(), controls.thinking.clone())
            .with_on_select_model(move |row| {
                if let Some(id) = tray_model_ids.get(row) {
                    (on_model_select.borrow_mut())(pane_id, id.clone());
                }
            })
            .with_on_select_level(move |level| {
                (on_thinking_select.borrow_mut())(pane_id, level);
            });
            chat = chat.with_composer_model_tray(tray);
        }
        // The approval policy is the native agent's too: a CLI harness decides
        // its own approvals, so the always-approve control would write a
        // setting nothing in the conversation reads.
        chat = chat
            .with_composer_approval_mode(controls.approval_mode)
            .with_composer_approval_menu_open(controls.approval_menu_open.clone())
            .with_composer_approval_menu_highlight(controls.approval_menu_highlight.clone());
    } else if session.worker.is_none() {
        // A custom harness owns the agent turn: its mark names what answers,
        // where the native agent would draw its model and approval controls.
        if let Some(spec) = surface_harness.filter(|spec| spec.id != "goble") {
            chat = chat.with_composer_agent_harness(spec.id);
        }
    }

    // Modal (vim) editing of the agent composer, when the user turned it on.
    if state.vim_mode {
        chat = chat.with_composer_vim(controls.vim.clone(), actions.clipboard.clone());
    }

    // The transcript's own footer: DSH's usage pill (what this conversation
    // has spent, from the provider's token counts — never a price) with its
    // click-open details panel, and the fork into a new one.
    {
        let on_fork = actions.on_fork_conversation.clone();
        chat = chat
            .with_usage(session.usage)
            .with_usage_detail(
                session.usage_last_call,
                session.usage_calls,
                session.usage_context_window,
            )
            .with_usage_open(session.usage_open.clone())
            .with_on_fork(move || (on_fork.borrow_mut())(pane_id));
    }

    // warp-new context pills: harness (the selected medium), working directory
    // (this pane's own path) and git branch. Each is a dropdown that routes its
    // selection back to this pane's controls, so the pills of two panes never
    // open or change together.
    if let Some(context) = state.composer_context.get(&pane_id) {
        let dir_ids = context.dir_ids.clone();
        chat = chat
            .with_composer_harness_label(context.harness_label.clone())
            .with_composer_harness_icon(context.harness_icon);
        // Only a conversation that runs on a worker chooses its environment
        // here; a local pane names the environment it already runs on and
        // draws no menu for it.
        if session.worker.is_some() {
            let harness_ids = context.harness_ids.clone();
            chat = chat.with_composer_harness_menu(
                context.harness_items.clone(),
                context.harness_menu_open.clone(),
                move |idx| {
                    if let Some(id) = harness_ids.get(idx) {
                        (on_select_harness.borrow_mut())(pane_id, id.clone());
                    }
                },
            );
        }
        chat = chat
            .with_composer_dir_menu(
                context.dir_items.clone(),
                context.dir_menu_open.clone(),
                move |idx| {
                    if let Some(id) = dir_ids.get(idx) {
                        (on_select_dir.borrow_mut())(pane_id, id.clone());
                    }
                },
            )
            .with_composer_dir_menu_scroll(context.dir_menu_scroll.clone())
            .with_composer_dir_menu_highlight(context.dir_menu_highlight.clone());
        if !context.branch_label.is_empty() {
            let branch_ids = context.branch_ids.clone();
            chat = chat
                .with_composer_branch_label(context.branch_label.clone())
                .with_composer_branch_menu(
                    context.branch_items.clone(),
                    context.branch_menu_open.clone(),
                    move |idx| {
                        if let Some(id) = branch_ids.get(idx) {
                            (on_select_branch.borrow_mut())(pane_id, id.clone());
                        }
                    },
                )
                .with_composer_branch_menu_highlight(context.branch_menu_highlight.clone());
        }
        if let Some(diff) = context.diff.clone() {
            let open = on_diff_panel.clone();
            chat = chat
                .with_composer_diff(diff, context.diff_menu_open.clone())
                .with_composer_diff_menu_scroll(context.diff_menu_scroll.clone())
                .with_composer_diff_open(move |file| {
                    (open.borrow_mut())(pane_id, crate::ui::DiffPanelAction::Open(file))
                });
        }
    }
    // The draft's own band: `/` lists the harness's commands, `$` the skills it
    // advertises and `@` the paths under the pane's working directory. All three
    // are the same band above the editor, narrowed by what was typed after the
    // prefix, and the list is the harness's own — never one written here.
    let draft_commands =
        crate::ui::palette::draft_commands(state, actions, &session.composer_draft);
    let draft_query = crate::ui::palette::draft_query(&session.composer_draft).unwrap_or_default();
    let slash_filtered = crate::ui::palette::matching_slash_commands(&draft_commands, &draft_query);
    let slash_items = crate::ui::palette::slash_items(&slash_filtered);
    let on_composer_insert = actions.on_composer_insert.clone();
    let mut chat = chat
        .with_composer_slash_menu(
            slash_items,
            controls.slash_index.clone(),
            controls.slash_dismissed.clone(),
        )
        .with_composer_on_slash_move(move |index| (on_slash_move.borrow_mut())(index))
        .with_composer_on_slash_accept(in_pane_with(
            &on_pane_activate,
            pane_id,
            crate::ui::palette::slash_accept(slash_filtered, on_slash_close, on_composer_insert),
        ))
        .with_composer_on_slash_dismiss(move || (on_slash_dismiss.borrow_mut())());
    // The way out of this view, for a pane that has one: the terminal pane's
    // harness returns to the shell, a chat pane becomes the terminal it has no
    // shell behind. A pane with neither passes `None`.
    if let Some(on_escape) = on_escape {
        chat = chat.with_on_escape(move || (on_escape.borrow_mut())());
    }
    // 341: the conversation's project pill sits above the composer. The side
    // panel's reserved conversation has no project of its own.
    if pane_id != crate::state::SIDE_AGENT_PANE_ID {
        let pill = crate::ui::chat::project_pill::build_project_pill(app, state, actions, pane_id);
        chat = chat.with_project_pill(pill);
    }
    // The right "agent panel" (routines + scheduled tasks) is now a floating
    // overlay layered over the whole app in `build_ui`, not a docked sidebar
    // that shrinks the pane. It is toggled from the chat header's "Toggle
    // panel" item and closed via its X button / backdrop click.
    //
    // The agent surface runs to the pane's edges: its own rows carry the
    // padding they need, and a gutter here read as a grey inset on both sides.
    let chat = chat.finish();
    // The bottom terminal dock: T3 Code's drawer, opened from the header icon.
    // It is a sibling under the whole chat view (transcript + composer), so the
    // agent surface is never replaced by a second one.
    if controls.terminal_dock_open && session.worker.is_none() {
        let dock = build_terminal_dock(app, state, actions, pane_id, active);
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Expanded::new(chat).finish())
            .with_child(dock)
            .finish()
    } else {
        chat
    }
}

#[cfg(test)]
mod transcript_scroll_tests {
    use super::*;
    use crate::state::UiState;
    use goble_ui::elements::{LayoutContext, SizeConstraint};
    use goble_ui::geometry::vec2f;
    use goble_ui::ScrollState;

    /// A transcript long enough to overflow the pane it is laid out in.
    fn transcript(count: usize) -> Vec<ChatMessage> {
        (0..count)
            .map(|i| {
                ChatMessage::new(
                    ChatRole::Assistant,
                    vec![ChatFragment::text(format!("streamed line {i}"))],
                )
            })
            .collect()
    }

    /// The transcript scroll state the app hands pane 1.
    fn pane_scroll(state: &UiState) -> Rc<RefCell<ScrollState>> {
        state
            .pane_chat_snapshot()
            .get(&1)
            .expect("pane 1 has chat data")
            .scroll
            .clone()
    }

    /// Lay the pane out at a fixed size, as the app does for every frame.
    fn layout_pane(view: &mut ChatView, app: &AppContext) {
        let _ = view.layout(
            SizeConstraint::loose(vec2f(600.0, 480.0)),
            &mut LayoutContext,
            app,
        );
    }

    #[test]
    fn a_new_transcript_opens_pinned_to_the_latest_message() {
        let app = AppContext::default();
        let mut state = UiState::mock();
        state.ensure_pane_controls();
        let scroll = pane_scroll(&state);
        let mut view = ChatView::new()
            .with_messages(transcript(60))
            .with_scroll_state(scroll.clone());
        layout_pane(&mut view, &app);
        assert!(
            scroll.borrow().max_offset() > 0.0,
            "60 messages overflow the pane"
        );
        assert_eq!(
            scroll.borrow().offset(),
            scroll.borrow().max_offset(),
            "the transcript opens at the end of the stream"
        );
    }

    #[test]
    fn new_content_does_not_move_a_scrollback_position_the_user_chose() {
        let app = AppContext::default();
        let mut state = UiState::mock();
        state.ensure_pane_controls();
        let scroll = pane_scroll(&state);
        let mut view = ChatView::new()
            .with_messages(transcript(60))
            .with_scroll_state(scroll.clone());
        layout_pane(&mut view, &app);

        // The user scrolls up to read earlier output.
        scroll.borrow_mut().scroll_by(-120.0);
        let held = scroll.borrow().offset();
        assert!(
            held < scroll.borrow().max_offset(),
            "the user left the end of the transcript"
        );

        // The agent streams on and the pane is rebuilt for the next frame.
        let mut view = ChatView::new()
            .with_messages(transcript(90))
            .with_scroll_state(scroll.clone());
        layout_pane(&mut view, &app);
        assert_eq!(
            scroll.borrow().offset(),
            held,
            "streaming must not move a scrollback position the user chose"
        );
    }

    #[test]
    fn returning_to_the_bottom_resumes_following_the_stream() {
        let app = AppContext::default();
        let mut state = UiState::mock();
        state.ensure_pane_controls();
        let scroll = pane_scroll(&state);
        let mut view = ChatView::new()
            .with_messages(transcript(60))
            .with_scroll_state(scroll.clone());
        layout_pane(&mut view, &app);
        scroll.borrow_mut().scroll_by(-120.0);

        // The user scrolls back to the bottom, so the stream is followed again.
        scroll.borrow_mut().scroll_by(10_000.0);
        assert!(scroll.borrow().is_pinned(), "back at the end re-pins");
        let mut view = ChatView::new()
            .with_messages(transcript(90))
            .with_scroll_state(scroll.clone());
        layout_pane(&mut view, &app);
        assert_eq!(
            scroll.borrow().offset(),
            scroll.borrow().max_offset(),
            "following resumed for the new content"
        );
    }
}

/// The pane's transcript is full-width rows, not pills: the agent's reply
/// paints no box, while the user's own message is DSH's capped bubble anchored
/// to the pane's trailing edge. The app's own chat surface is rendered over a
/// real store, so the geometry is asserted where the user sees it.
#[cfg(test)]
mod chat_geometry_tests {
    use super::*;
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::AppContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::theme::ColorToken;
    use std::sync::Arc;

    /// The drawn origin of the pane's own run `label` (a run left of the
    /// sidebar is the sidebar quoting the conversation, not the transcript).
    fn pane_run_origin(commands: &[RenderCommand], label: &str) -> Option<(f32, f32)> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if text == label && origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some((origin.x, origin.y))
            }
            _ => None,
        })
    }

    #[test]
    fn the_panes_transcript_paints_a_right_aligned_user_bubble() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let chat_id = desktop
            .create_chat("Demo", None, None)
            .expect("create chat");
        desktop
            .add_chat_message(&chat_id, "user", "Salut!")
            .expect("user row");
        desktop
            .add_chat_message(&chat_id, "assistant", "Bine ai venit!")
            .expect("assistant row");

        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(RootView::new(&app, &desktop, None));
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

        let (user_x, user_y) =
            pane_run_origin(&commands, "Salut!").expect("the user's message is drawn");
        let (reply_x, _) =
            pane_run_origin(&commands, "Bine ai venit!").expect("the agent's reply is drawn");

        // The pane's content column starts and ends past the shell, sidebar and
        // frame insets.
        let pane_left = crate::ui::shell::WORKSPACE_MARGIN
            + crate::ui::SIDEBAR_WIDTH
            + crate::ui::panes::PANE_AREA_INSET
            + crate::ui::panes::PANE_INSET;
        let pane_right = 1024.0
            - crate::ui::shell::WORKSPACE_MARGIN
            - crate::ui::panes::PANE_AREA_INSET
            - crate::ui::panes::PANE_INSET;

        // The user's own message: the rounded bubble behind its text, anchored
        // to the pane's trailing edge and narrower than the row.
        let band = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && rect.min_x() <= user_x
                    && user_x <= rect.max_x()
                    && rect.min_y() <= user_y
                    && user_y <= rect.max_y() =>
                {
                    Some((*rect, *corner_radius))
                }
                _ => None,
            })
            .expect("the user's message paints its bubble");
        assert_eq!(
            band.1,
            goble_ui::elements::chat_typography::USER_BUBBLE_RADIUS,
            "the user's bubble carries DSH's 20 px radius"
        );
        assert_eq!(
            band.0.max_x(),
            pane_right - 16.0,
            "the user's bubble keeps a 16 pt inset from the lane's trailing edge"
        );
        assert!(
            band.0.min_x() > pane_left + 50.0,
            "the bubble is capped, not a pane-wide band: {} against {pane_left}",
            band.0.min_x()
        );
        assert!(
            (user_x - (band.0.min_x() + 16.0)).abs() < 0.01,
            "the user's text keeps the bubble's 16 px inset: {user_x} against {:?}",
            band.0
        );

        let padded = reply_x - pane_left;
        assert!(
            (11.0..=17.0).contains(&padded),
            "the agent's row keeps its band inset after the top/left pane band was removed, got {padded}"
        );

        // No prompt marker anywhere: the user's row is a bubble, not a marker
        // plus a band.
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "prompt-arrow"
            )),
            "the user's message draws no prompt marker"
        );
    }
}

#[cfg(test)]
mod link_action_tests {
    use super::*;
    use goble_core::harness::ToolCallStatus;
    use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::event::DispatchedEvent;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::Element;
    use goble_ui::{SubAgentRow, SubAgentRowStatus, ToolCall};
    use std::collections::HashMap;
    use std::time::Duration;

    /// The app-side opener stand-in: the guarded entry point, with the platform
    /// launcher replaced by a recorder so a click test never starts a browser.
    fn guarded_opener(
        launched: Rc<RefCell<Vec<String>>>,
        refused: Rc<RefCell<Vec<String>>>,
    ) -> Rc<RefCell<dyn FnMut(String)>> {
        Rc::new(RefCell::new(move |url: String| {
            let launched = launched.clone();
            let refused = refused.clone();
            if let Err(e) = crate::state::open_external_url_with(&url, &move |u: &str| {
                launched.borrow_mut().push(u.to_string());
                Ok(())
            }) {
                refused.borrow_mut().push(e);
            }
        }))
    }

    /// Build the pane's transcript, lay it out, and click each run `label` in
    /// turn, repainting between clicks so a row revealed by an earlier click is
    /// a target of the next one.
    fn click_message(
        message: ChatMessage,
        on_open_url: Rc<RefCell<dyn FnMut(String)>>,
        label: &str,
    ) {
        click_transcript(message, on_open_url, &[label])
    }

    /// The same clicks through a transcript that dispatches its actions to
    /// `on_open_url`.
    fn click_transcript(
        message: ChatMessage,
        on_open_url: Rc<RefCell<dyn FnMut(String)>>,
        labels: &[&str],
    ) {
        let app = AppContext::default();
        let mut view =
            ChatView::new()
                .with_messages(vec![message])
                .with_on_action(chat_action_relay(
                    on_open_url,
                    Rc::new(RefCell::new(|_: String| {})),
                    Rc::new(RefCell::new(|_: String| {})),
                    None,
                    None,
                ));
        for label in labels {
            view.layout(
                SizeConstraint::loose(vec2f(600.0, 400.0)),
                &mut LayoutContext,
                &app,
            );
            let mut paint_ctx = PaintContext::new(Renderer::new());
            view.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
            let commands = paint_ctx
                .renderer
                .take()
                .map(|r| r.commands().to_vec())
                .unwrap_or_default();
            let origin = commands
                .iter()
                .find_map(|c| match c {
                    RenderCommand::DrawText { origin, text, .. } if text == *label => Some(*origin),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the transcript must draw the run {label:?}"));
            let position = origin + vec2f(2.0, 4.0);
            let mut ctx = EventContext;
            for event in [
                DispatchedEvent::MouseDown {
                    position,
                    button: 0,
                },
                DispatchedEvent::MouseUp {
                    position,
                    button: 0,
                },
            ] {
                view.dispatch_event(&event, &mut ctx, &app);
            }
        }
    }

    /// R7: the click terminates in the app's opener, carrying the link's URL.
    #[test]
    fn a_link_click_reaches_the_app_opener_with_its_url() {
        let launched = Rc::new(RefCell::new(Vec::new()));
        let refused = Rc::new(RefCell::new(Vec::new()));
        let message = ChatMessage::from_markdown(
            ChatRole::Assistant,
            "see [Goble](https://goble.dev) for details",
        );
        click_message(
            message,
            guarded_opener(launched.clone(), refused.clone()),
            "Goble",
        );
        assert_eq!(
            *launched.borrow(),
            vec!["https://goble.dev".to_string()],
            "a link click must reach the opener carrying its URL"
        );
        assert!(refused.borrow().is_empty(), "an https URL is not refused");
    }

    /// R7: a non-http(s) link reaches the app slot but is refused by the scheme
    /// guard before the opener, so nothing is launched.
    #[test]
    fn a_non_http_scheme_click_is_refused_before_the_opener() {
        let launched = Rc::new(RefCell::new(Vec::new()));
        let refused = Rc::new(RefCell::new(Vec::new()));
        let message = ChatMessage::new(
            ChatRole::Assistant,
            vec![
                ChatFragment::text("see "),
                ChatFragment::link("unsafe", "file:///etc/passwd"),
            ],
        );
        click_message(
            message,
            guarded_opener(launched.clone(), refused.clone()),
            "unsafe",
        );
        assert!(
            launched.borrow().is_empty(),
            "a non-http(s) link must never reach the opener"
        );
        assert_eq!(
            refused.borrow().len(),
            1,
            "the click must be refused by the scheme guard"
        );
    }

    /// The live sub-agent row is a status line in the transcript: the controls
    /// that enter and stop the child belong to the pane's strip above it, so
    /// the reply draws neither. The row still names the child's description and
    /// the model the pane runs on.
    #[test]
    fn a_sub_agent_row_draws_no_controls_in_the_transcript() {
        let app = AppContext::default();
        let message = ChatMessage::new(ChatRole::Assistant, vec![])
            .with_tool_calls(vec![spawn_call(ToolCallStatus::Running)]);
        let mut view = ChatView::new()
            .with_messages(vec![message])
            .with_sub_agents(HashMap::from([(
                "call_spawn".to_string(),
                running_record(),
            )]))
            .with_sub_agent_model("deepseek");
        view.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext,
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        view.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();
        let texts: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts
                .iter()
                .any(|text| text.contains("running: “audit the migration”")),
            "the row names the delegation, got {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|text| text.contains("· Responding (deepseek)")),
            "the row names the pane's model, got {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|text| text == "Stop" || text == "open child"),
            "the transcript row draws no controls, got {texts:?}"
        );
    }

    /// The spawn call the row tests click.
    fn spawn_call(status: ToolCallStatus) -> ToolCall {
        ToolCall {
            id: "call_spawn".to_string(),
            name: "spawn_subagent".to_string(),
            arguments: r#"{"subagent_type":"reviewer","input":"audit the migration"}"#.to_string(),
            status,
            result: None,
            diff: None,
        }
    }

    /// A live record for that spawn call's child.
    fn running_record() -> SubAgentRow {
        SubAgentRow {
            child_id: "conv-child-1".to_string(),
            subagent_type: "reviewer".to_string(),
            description: "audit the migration".to_string(),
            status: SubAgentRowStatus::Running,
            activity: "reading the schema".to_string(),
            elapsed: Duration::from_millis(4500),
            turns: 2,
            tool_calls: 4,
            tokens: 1234,
            outcome: None,
            background: true,
        }
    }
}

/// R1's pill lock, on the pane the app mounts: the agent's own surface draws no
/// pill. The root is rendered over a real store with a live transcript — the
/// user's message, the agent's prose, a tool call still in flight, a finished
/// tool call and a running sub-agent — and no rounded box or border may cover
/// any of those rows' own runs. The shared terminal command block is the one
/// bordered box the transcript draws, and it is the same block the terminal
/// pane draws for the same command.
#[cfg(test)]
mod agent_pill_tests {
    use super::*;
    use crate::root_view::RootView;
    use crate::state::{PaneRuntime, SubAgentRecord};
    use goble_core::harness::ToolCallStatus;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::AppContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::{pill_rects, rect_contains, render_element};
    use goble_ui::{SubAgentRow, SubAgentRowStatus, ToolCall};
    use std::sync::Arc;
    use std::time::Duration;

    /// The pane's own run `label`, as drawn: a run left of the sidebar is the
    /// sidebar quoting the conversation, not the transcript.
    fn pane_run_origin(commands: &[RenderCommand], label: &str) -> Option<goble_ui::Vector2F> {
        commands.iter().find_map(|command| match command {
            // The row's operand may share its drawn run with a detail in the
            // same colour (a folded edit's diffstat), so the labels match the
            // run's head.
            RenderCommand::DrawText { origin, text, .. }
                if text.starts_with(label) && origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some(*origin)
            }
            _ => None,
        })
    }

    #[test]
    fn the_agent_pane_paints_no_pill_around_its_rows() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();

        // A finished read, a finished edit that failed, and the two calls a
        // turn has in flight: a search and a spawned child.
        let read = ToolCall {
            id: "call_read".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"src/sidebar.rs"}"#.to_string(),
            status: ToolCallStatus::Finished,
            result: Some("1: fn main() {}".to_string()),
            diff: None,
        };
        let edit = ToolCall {
            id: "call_edit".to_string(),
            name: "edit_file".to_string(),
            arguments: r#"{"path":"src/panes.rs","old_text":"let a = 1;","new_text":"let a = 2;"}"#
                .to_string(),
            status: ToolCallStatus::Error,
            result: Some("old_text not found".to_string()),
            diff: None,
        };
        let search = ToolCall {
            id: "call_search".to_string(),
            name: "codebase_search".to_string(),
            arguments: r#"{"pattern":"fn build","path":"src"}"#.to_string(),
            status: ToolCallStatus::Running,
            result: None,
            diff: None,
        };
        let spawn = ToolCall {
            id: "call_spawn".to_string(),
            name: "spawn_subagent".to_string(),
            arguments: r#"{"subagent_type":"reviewer","input":"audit the migration"}"#.to_string(),
            status: ToolCallStatus::Running,
            result: None,
            diff: None,
        };

        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.active_pane_id = 1;
            let mut runtime = PaneRuntime {
                busy: true,
                messages: vec![
                    ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Fix the build")]),
                    ChatMessage::new(
                        ChatRole::Assistant,
                        vec![ChatFragment::text("Reading the sidebar, then the panes.")],
                    )
                    .with_tool_calls(vec![
                        read.clone(),
                        edit.clone(),
                        search.clone(),
                        spawn.clone(),
                    ]),
                ],
                ..PaneRuntime::default()
            };
            runtime
                .in_flight_tools
                .insert(search.id.clone(), search.clone());
            runtime.sub_agents.insert(
                "conv-child-1".to_string(),
                SubAgentRecord {
                    parent_call_id: spawn.id.clone(),
                    row: SubAgentRow {
                        child_id: "conv-child-1".to_string(),
                        subagent_type: "reviewer".to_string(),
                        description: "audit the migration".to_string(),
                        status: SubAgentRowStatus::Running,
                        activity: "reading the schema".to_string(),
                        elapsed: Duration::from_millis(4500),
                        turns: 2,
                        tool_calls: 4,
                        tokens: 1234,
                        outcome: None,
                        background: true,
                    },
                },
            );
            // The groupable calls start folded into their runs; opening each
            // run is what draws the member rows this lock measures. The fold
            // map is the same app-owned one a header click writes.
            s.tool_fold.borrow_mut().insert(
                format!("verb-group:{}", goble_ui::tool_fold_key(&read, 0)),
                goble_ui::ToolDisplayMode::Expanded,
            );
            s.tool_fold.borrow_mut().insert(
                format!("verb-group:{}", goble_ui::tool_fold_key(&search, 2)),
                goble_ui::ToolDisplayMode::Expanded,
            );
            s.pane_runtime.insert(1, runtime);
        }

        let mut root: Box<dyn Element> = Box::new(view);
        // Taller than the window so every row this lock measures is on screen.
        let commands = render_element(&mut root, vec2f(1024.0, 1400.0), &app);

        // The rows the transcript drew, as the runs they own. A pill is a
        // rounded box (or a border) drawn over the row its run belongs to.
        // The user's rounded bubble is the deliberate exception to the no-pill
        // rule, so the measured rows start at the agent's reply.
        let row_labels = [
            "Reading the sidebar, then the panes.",
            "Read src/sidebar.rs",
            "Edit src/panes.rs",
            "Search \"fn build\" in src",
            // The transcript names a spawned child by what it is: `Agent`,
            // the word grok-build gives the row, not the tool's own name.
            "Agent ",
        ];
        let rows: Vec<goble_ui::Vector2F> = row_labels
            .iter()
            .filter_map(|label| pane_run_origin(&commands, label))
            .collect();
        assert_eq!(
            rows.len(),
            row_labels.len(),
            "every row the lock measures must be drawn, got {rows:?}"
        );

        // The pane no longer paints a casing of its own: the shell's rounded
        // workspace surface encloses the rows, so there is no pane frame to
        // mistake for a row pill. Every remaining pill is a row's own box.
        let pills: Vec<goble_ui::RectF> = pill_rects(&commands)
            .into_iter()
            .filter(|pill| !rows.iter().all(|row| rect_contains(*pill, *row)))
            .collect();
        for label in row_labels {
            let Some(origin) = pane_run_origin(&commands, label) else {
                continue;
            };
            for pill in &pills {
                assert!(
                    !rect_contains(*pill, origin),
                    "the agent row {label:?} must not be covered by a pill: {pill:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod conversation_command_tests {
    use super::*;
    use crate::emulator::VisibleBlock;
    use goble_terminal::{BlockId, BlockOwner, BlockState};
    use goble_ui::elements::ChatFragmentKind;

    fn block(owner: BlockOwner, command: &str) -> VisibleBlock {
        VisibleBlock {
            id: BlockId(1),
            owner,
            command: command.to_string(),
            output: "out".to_string(),
            state: BlockState::DoneWithExecution,
            exit_code: Some(0),
            pwd: Some("/tmp".to_string()),
            git_branch: None,
            duration: None,
            started_at: None,
            prompt: None,
            card: None,
        }
    }

    /// The commands the conversation ran with `!` are the rows it adds, drawn
    /// through the shared terminal block. A block the agent owns is left out:
    /// its turn already carries that command as a tool-call row.
    #[test]
    fn a_conversations_own_commands_are_the_rows_it_adds() {
        let typed = block(BlockOwner::User, "pwd");
        let ran_by_the_agent = block(
            BlockOwner::Agent {
                conversation_id: "c1".to_string(),
                call_id: "call-1".to_string(),
            },
            "echo hi",
        );

        let rows = conversation_command_rows(&[typed, ran_by_the_agent]);

        assert_eq!(rows.len(), 1, "a command is one row");
        let row = &rows[0].1;
        assert_eq!(row.role, ChatRole::Tool);
        match &row.fragments[0].kind {
            ChatFragmentKind::Terminal(data) => {
                assert_eq!(data.lines[0].text, "pwd", "the row is the command's block");
                assert_eq!(
                    data.meta.as_ref().map(|meta| meta.path.as_str()),
                    Some("/tmp"),
                    "the block carries where it ran"
                );
            }
            other => panic!("the row is the shared terminal block, got {other:?}"),
        }
    }

    /// Nothing to add is no row: a pane with no conversation of its own draws
    /// no command rows.
    #[test]
    fn a_pane_without_a_conversation_adds_no_rows() {
        assert!(conversation_command_rows(&[]).is_empty());
    }

    /// A command typed mid-conversation keeps the place it was sent in: it sits
    /// after the reply it followed and before the reply the agent wrote after
    /// it, rather than floating at the end of the transcript.
    #[test]
    fn a_command_is_placed_where_it_started() {
        let at = |secs: u64| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        let stamped = |role, text: &str, secs: u64| {
            let time = chrono::DateTime::<chrono::Utc>::from(at(secs)).to_rfc3339();
            ChatMessage::new(role, vec![ChatFragment::text(text)]).with_timestamp(time)
        };
        let messages = vec![
            stamped(ChatRole::User, "first", 100),
            stamped(ChatRole::Assistant, "reply one", 110),
            stamped(ChatRole::User, "second", 200),
            stamped(ChatRole::Assistant, "reply two", 210),
        ];
        let commands = vec![
            (
                Some(at(150)),
                ChatMessage::new(ChatRole::Tool, vec![ChatFragment::text("!mid")]),
            ),
            (
                Some(at(250)),
                ChatMessage::new(ChatRole::Tool, vec![ChatFragment::text("!late")]),
            ),
        ];

        let merged = merge_conversation_commands(messages, commands);
        let order: Vec<String> = merged
            .iter()
            .filter_map(|message| match message.fragments.first().map(|f| &f.kind) {
                Some(ChatFragmentKind::Text(text)) => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            order,
            vec!["first", "reply one", "!mid", "second", "reply two", "!late"],
            "the transcript keeps the order the turn happened in"
        );
    }

    /// A row with nothing to compare (a live reasoning block) keeps its place,
    /// and a command with no start time stays at the end rather than forcing
    /// the whole transcript to reorder around it.
    #[test]
    fn an_undated_row_keeps_its_place() {
        let at = |secs: u64| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        let messages = vec![
            ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("live")]),
            ChatMessage::new(ChatRole::User, vec![ChatFragment::text("next")])
                .with_timestamp(chrono::DateTime::<chrono::Utc>::from(at(200)).to_rfc3339()),
        ];
        let commands = vec![(
            None,
            ChatMessage::new(ChatRole::Tool, vec![ChatFragment::text("!undated")]),
        )];

        let merged = merge_conversation_commands(messages, commands);
        assert_eq!(
            merged
                .iter()
                .filter_map(|message| match message.fragments.first().map(|f| &f.kind) {
                    Some(ChatFragmentKind::Text(text)) => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["live", "next", "!undated"]
        );
    }
}

/// End-to-end wiring of the agent composer's controls through the real app
/// tree: the gesture the user makes on a pane's composer must act on *that*
/// pane, through the real actions and the real store, not on whichever pane was
/// active before.
#[cfg(test)]
mod composer_wiring_tests {
    use super::*;
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{Element, EventContext};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::vec2f;
    use goble_ui::platform::WindowControl;
    use goble_ui::render::RenderCommand;

    use crate::actions::make_actions;
    use crate::media::MediaState;
    use crate::root_view::RootView;
    use crate::state::UiState;
    use crate::ui::UiActions;

    /// The window the app is laid out at: the pane area is split in two, so a
    /// control on the left pane can be used while the right one is active.
    const WINDOW: (f32, f32) = (1024.0, 768.0);

    fn app() -> AppContext {
        AppContext::default()
    }

    fn render(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        goble_ui::test_util::render_element(root, vec2f(WINDOW.0, WINDOW.1), app)
    }

    /// A configured window with two panes: pane 1 (a chat pane) holding the
    /// draft "hello from pane one", and pane 3 (the split pane) active. The
    /// app's own actions are returned so a test can move the active pane the
    /// way a press on the other pane's body does.
    fn split_window() -> (
        Box<dyn Element>,
        Rc<RefCell<UiState>>,
        Arc<DesktopState>,
        tempfile::TempDir,
        UiActions,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        let root = RootView::new(&app(), &desktop, None);
        let state = root.state_rc();
        let pane_one_chat = desktop
            .create_chat("Pane one", None, None)
            .expect("create pane one's chat");
        {
            let mut s = state.borrow_mut();
            // The app runs as a configured window, so a submit has somewhere
            // to send the prompt: the mock provider answers it.
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.settings_llm_provider = "mock".to_string();
            s.settings_llm_api_key = "test-key".to_string();
            s.selected_model = "mock-model".to_string();
            s.record_pane_conversation(1, &pane_one_chat);
            if let Some(session) = s.pane_sessions.get_mut(&1) {
                session.draft = "hello from pane one".to_string();
            }
        }
        let media = Rc::new(RefCell::new(MediaState::mock()));
        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            media,
            WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );
        // The split opens a second pane and makes it the active one.
        (actions.on_split_right.borrow_mut())();
        assert_eq!(state.borrow().active_pane_id, 3, "the split pane is active");
        (
            Box::new(root) as Box<dyn Element>,
            state,
            desktop,
            dir,
            actions,
        )
    }

    fn conversation(state: &Rc<RefCell<UiState>>, pane_id: u64) -> String {
        state
            .borrow()
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.conversation_id.clone())
            .expect("the pane owns a conversation")
    }

    fn wait_for_messages(desktop: &DesktopState, conversation: &str, count: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while desktop
            .list_chat_messages(conversation)
            .map(|messages| messages.len())
            .unwrap_or(0)
            < count
        {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {count} messages in {conversation}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn enter() -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState::none(),
        }
    }

    /// A keyboard submit from the composer that still holds the caret after the
    /// user pressed the *other* pane's body names the composer's own pane: the
    /// gesture is dispatched before the frame that would unfocus the moved
    /// pane, so the composer itself has to say which pane it belongs to.
    ///
    /// Without that, the draft typed in the left pane was sent to the right
    /// pane's conversation — the active one — and the wrong draft was cleared.
    #[test]
    fn a_submit_after_the_active_pane_moved_names_the_composers_own_pane() {
        let (mut element, state, desktop, _dir, actions) = split_window();
        let app = app();
        let pane_one_chat = conversation(&state, 1);
        let pane_three_chat = conversation(&state, 3);
        // The left pane holds the caret when the frame is drawn...
        {
            let mut s = state.borrow_mut();
            s.active_pane_id = 1;
            s.composer_focused = true;
        }
        let _ = render(&mut element, &app);
        // ...and the user then presses the right pane's body, which moves the
        // app's active pane before the next redraw reaches the tree.
        (actions.on_pane_activate.borrow_mut())(3);
        assert_eq!(state.borrow().active_pane_id, 3, "the press moved the pane");

        let runtime = tokio::runtime::Runtime::new().expect("build runtime");
        let _guard = runtime.enter();
        let mut ctx = EventContext;
        assert!(
            element.dispatch_event(&enter(), &mut ctx, &app),
            "the focused composer takes Enter"
        );
        assert_eq!(
            state.borrow().active_pane_id,
            1,
            "the submit names the pane whose composer it came from"
        );
        wait_for_messages(&desktop, &pane_one_chat, 2);
        assert_eq!(
            desktop
                .list_chat_messages(&pane_one_chat)
                .expect("list pane one's messages")
                .len(),
            2,
            "the left pane's turn ran in its own conversation"
        );
        assert!(
            desktop
                .list_chat_messages(&pane_three_chat)
                .expect("list pane three's messages")
                .is_empty(),
            "the pane that was active instead is untouched"
        );
        assert!(
            state
                .borrow()
                .pane_sessions
                .get(&1)
                .is_some_and(|session| session.draft.is_empty()),
            "the submitted draft is spent"
        );
    }
}
