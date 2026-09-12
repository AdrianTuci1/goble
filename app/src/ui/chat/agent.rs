use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Element, PopupMenuItem,
};
use goble_ui::{ChatFragment, ChatMessage, ChatRole, ChatView};

use crate::state::PaneControls;

use super::super::{UiActions, UiSnapshot};
use super::{build_agent_error, build_agent_header, build_sub_agent_child_view, chat_action_relay, pane_session_snapshot};

/// If `pane_id` has a live PTY session with output, append it as an inline
/// terminal block after the transcript so a terminal command (`!cmd`) actually
/// shows its output (warp-new style executed-command block). The block is
/// recomputed each frame from the session buffer, so a long-running command
/// streams as it runs without ever growing the stored transcript.
fn with_inline_terminal(
    mut messages: Vec<ChatMessage>,
    terminal: &std::rc::Rc<std::cell::RefCell<crate::terminal::TerminalRegistry>>,
    pane_id: u64,
) -> Vec<ChatMessage> {
    // The pane's own executed-command block: the transcript draws it with the
    // same terminal block the pane does, so one command is one block.
    let block = {
        let reg = terminal.borrow();
        reg.sessions
            .get(&pane_id)
            .and_then(crate::ui::terminal::executed_command_block)
    };
    if let Some(data) = block {
        messages.push(ChatMessage::new(
            ChatRole::Tool,
            vec![ChatFragment::terminal(data)],
        ));
    }
    messages
}

/// Agent chat tab: a header row with the agent identity/status/copy/restart,
/// then the message transcript + composer (which fills the remaining space).
///
/// `pane_id` selects this pane's own transcript + composer draft + path, so
/// every chat pane renders an independent session instead of the global one.
///
/// `on_escape` is the transcript's Escape hook: a pane whose agent view is the
/// way back out of another surface (the terminal pane's harness) passes its
/// leave handler; a chat pane passes `None`.
pub fn build_agent_chat(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    active: bool,
    on_escape: Option<Rc<RefCell<dyn FnMut()>>>,
) -> Box<dyn Element> {
    let session = pane_session_snapshot(state, pane_id);

    // A pane showing a sub-agent's child view (S6) shows that child's own
    // conversation, not its own: the child's transcript under a title naming the
    // child, and no composer of the parent's.
    if let Some(child) = session.sub_agent_view.clone() {
        return build_sub_agent_child_view(app, state, actions, pane_id, active, &session, &child);
    }

    // A pane whose harness is open *is* this pane's agent surface: the terminal
    // pane reaches it through `build_terminal`'s escape closure, and it is the
    // only caller that wires one.
    let harness_open = on_escape.is_some();
    let header = build_agent_header(app, state, actions, pane_id, harness_open);

    // The rich-input controls belong to this pane, not to the workspace: two
    // pty/agent panes side by side keep their own model, auto-approve switch,
    // branch and dropdown open flags.
    let controls = state.pane_controls.get(&pane_id).cloned().unwrap_or_else(|| {
        PaneControls::new(state.selected_model.clone(), state.auto_approve, String::new())
    });

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_pane_activate = actions.on_pane_activate.clone();
    let on_send_message = actions.on_send_message.clone();
    let on_attach = actions.on_attach.clone();
    let on_voice = actions.on_voice.clone();
    let on_select_model = actions.on_select_model.clone();
    let on_stop = actions.on_stop.clone();
    let on_answer_ask = actions.on_answer_ask.clone();
    let on_skip_ask = actions.on_skip_ask.clone();
    let on_command_decision = actions.on_command_decision.clone();
    let on_toggle_auto_approve = actions.on_toggle_auto_approve.clone();
    let on_send_queued = actions.on_send_queued.clone();
    let on_dismiss_queued = actions.on_dismiss_queued.clone();
    let on_close_inline_screen = actions.on_close_inline_screen.clone();
    let on_open_screen_link = actions.on_open_screen_link.clone();
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
    let on_select_harness = actions.on_select_harness.clone();
    let on_select_dir = actions.on_select_dir.clone();
    let on_select_branch = actions.on_select_branch.clone();
    let on_composer_slash = actions.on_composer_slash.clone();
    let on_cmd_enter = actions.on_cmd_enter.clone();

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

    // Append any live terminal-command output the pane's PTY has produced, so
    // a `!cmd` run from the chat pane shows its output inline.
    //
    // Not for the terminal pane's harness view: that view is opened *from* the
    // shell, and the shell's own commands already are the pane's history behind
    // it. Carrying them into the conversation would open a new chat with the
    // terminal's screen pasted into it. A pane that came from the terminal
    // starts its conversation on a clean sheet.
    let messages = if harness_open {
        session.messages.clone()
    } else {
        with_inline_terminal(session.messages.clone(), &state.terminal, pane_id)
    };
    let mut chat = ChatView::new()
        .with_header(header)
        // No key configured: the error shows at the top of this pane's chat
        // area, with the button that opens the model-provider dialog.
        .with_notice(
            state
                .show_llm_key_banner
                .then(|| build_agent_error(app, actions)),
        )
        .with_messages(messages)
        // The transcript scrolls and follows the stream; the state is
        // app-owned per pane so a chosen scrollback position is held.
        .with_scroll_state(session.scroll.clone())
        // Per-block terminal filter state is app-owned (shared) so the filter
        // tray's open flag + selection survive the per-frame rebuild; the copy
        // handler copies a terminal block's text to the clipboard.
        .with_terminal_filters(state.terminal_filters.clone())
        // The app-owned reasoning expand map: a thinking row the user opened
        // survives the per-frame rebuild.
        .with_reasoning_expanded(state.reasoning_expanded.clone())
        // The app-owned tool-call fold map: a folded call survives the rebuild.
        .with_tool_fold(state.tool_fold.clone())
        // This pane's own filter bar state (seeded per pane before the
        // snapshot), so opening it in one pane leaves the siblings alone.
        .with_global_terminal_filter(state.terminal_global_filters.get(&pane_id).cloned())
        .with_on_copy_terminal(Some(actions.on_copy_terminal.clone()))
        // The live sub-agent records for this pane: the transcript's rows read
        // their status, activity and elapsed time from these, not from the
        // spawn call's arguments.
        .with_sub_agents(session.sub_agents.clone())
        // A link inside a paragraph dispatches `ChatAction::OpenUrl`; a
        // sub-agent row dispatches `ChatAction::OpenSubAgent`. This is the slot
        // that carries them out of the view into the app's handlers.
        .with_on_action(chat_action_relay(on_open_url, on_open_sub_agent))
        .with_composer_value(session.composer_draft.clone())
        // Only the active pane's composer is focused, so a background chat pane
        // never swallows keys before the active pane (e.g. a terminal) sees them.
        .with_composer_focused(active && state.composer_focused)
        .with_composer_caret(controls.caret.clone())
        // The transcript's own fold key (`e`) is the active pane's too.
        .with_pane_active(active)
        .with_composer_path(crate::state::display_path(&session.composer_path))
        .with_composer_model_label(controls.model.clone())
        .with_composer_stop_visible(session.agent_busy)
        // The live turn-status footer, fed by C1's real live state per pane. It
        // is information only, so it carries no click handler.
        .with_turn_status(session.turn_status.clone())
        .with_composer_on_change(move |text| (on_composer_change.borrow_mut())(text))
        // Focusing a pane's composer also activates that pane, so the composer
        // draft / send route to the pane the user is actually typing in.
        .with_composer_on_focus_change(move |focused| {
            if focused {
                (on_pane_activate.borrow_mut())(pane_id);
            }
            (on_composer_focus.borrow_mut())(focused);
        })
        .with_composer_on_attach(move || (on_attach.borrow_mut())())
        .with_composer_on_voice(move || (on_voice.borrow_mut())())
        .with_composer_on_select_model(move || (on_select_model.borrow_mut())())
        .with_composer_on_stop(move || (on_stop.borrow_mut())())
        .with_pending_ask(session.pending_ask.clone())
        .with_on_answer_ask(move |resp, cred| (on_answer_ask.borrow_mut())(resp, cred))
        .with_on_skip_ask(move || (on_skip_ask.borrow_mut())())
        .with_command_proposal(session.pending_command.clone())
        .with_command_proposal_selection(session.command_selection.clone())
        .with_on_command_decision(move |id, decision| {
            (on_command_decision.borrow_mut())(pane_id, id, decision)
        })
        .with_auto_approve(controls.auto_approve)
        .with_on_toggle_auto_approve(move |on| (on_toggle_auto_approve.borrow_mut())(pane_id, on))
        .with_queued_prompt(session.queued_prompt.clone())
        .with_on_send_queued(move || (on_send_queued.borrow_mut())())
        .with_on_dismiss_queued(move || (on_dismiss_queued.borrow_mut())())
        .with_screen_link(session.screen_link.clone())
        .with_on_open_screen_link(move |uri| (on_open_screen_link.borrow_mut())(uri))
        .with_on_close_inline_screen(move || (on_close_inline_screen.borrow_mut())())
        .with_composer_model_menu(model_items, model_menu_open, move |index| {
            if let Some(name) = models_for_select.get(index) {
                (on_model_select.borrow_mut())(pane_id, name.clone());
            }
        })
        .with_on_send(move |text| (on_send_message.borrow_mut())(text))
        .with_composer_on_cmd_enter(move |text| (on_cmd_enter.borrow_mut())(text));

    // Modal (vim) editing of the agent composer, when the user turned it on.
    if state.vim_mode {
        chat = chat.with_composer_vim(controls.vim.clone(), actions.clipboard.clone());
    }

    // The transcript's own footer: what this conversation has spent (the
    // provider's token counts, never a price), and the fork into a new one.
    {
        let on_fork = actions.on_fork_conversation.clone();
        chat = chat
            .with_usage(session.usage)
            .with_usage_open(session.usage_open.clone())
            .with_on_fork(move || (on_fork.borrow_mut())(pane_id));
    }

    // A live remote-desktop handoff renders inline at the end of the
    // transcript. The frame's texture key is stable per pane so the screen
    // stream updates in place across rebuilds.
    if let Some(frame) = &session.inline_screen {
        chat = chat.with_inline_screen(
            format!("inline-{pane_id}"),
            frame.frame_seq,
            frame.width,
            frame.height,
            std::sync::Arc::clone(&frame.data),
        );
    }

    // warp-new context pills: harness (the selected medium), working directory
    // (this pane's own path) and git branch. Each is a dropdown that routes its
    // selection back to this pane's controls, so the pills of two panes never
    // open or change together.
    if let Some(context) = state.composer_context.get(&pane_id) {
        let harness_ids = context.harness_ids.clone();
        let dir_ids = context.dir_ids.clone();
        chat = chat
            .with_composer_harness_label(context.harness_label.clone())
            .with_composer_harness_menu(
                context.harness_items.clone(),
                context.harness_menu_open.clone(),
                move |idx| {
                    if let Some(id) = harness_ids.get(idx) {
                        (on_select_harness.borrow_mut())(pane_id, id.clone());
                    }
                },
            )
            .with_composer_dir_menu(
                context.dir_items.clone(),
                context.dir_menu_open.clone(),
                move |idx| {
                    if let Some(id) = dir_ids.get(idx) {
                        (on_select_dir.borrow_mut())(pane_id, id.clone());
                    }
                },
            );
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
                );
        }
    }
    let mut chat = chat.with_composer_on_slash(move || (on_composer_slash.borrow_mut())());
    // The way out of this view, for a pane that has one (the terminal pane's
    // harness returns to the shell). A chat pane wires nothing here.
    if let Some(on_escape) = on_escape {
        chat = chat.with_on_escape(move || (on_escape.borrow_mut())());
    }
    let chat = chat.finish();

    // The right "agent panel" (routines + scheduled tasks) is now a floating
    // overlay layered over the whole app in `build_ui`, not a docked sidebar
    // that shrinks the pane. It is toggled from the chat header's "Toggle
    // panel" item and closed via its X button / backdrop click.
    chat
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
            &mut LayoutContext::default(),
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

/// R1: the pane's transcript is full-width rows, not pills. The app's own chat
/// surface is rendered over a real store, so the geometry is asserted where the
/// user sees it: the user's message is one square band running to the pane's
/// edges, the agent's reply starts at the same edge with no box of its own, and
/// both keep the transcript's 10-15 px of padding.
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
    fn the_panes_transcript_paints_full_width_rows() {
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
        assert_eq!(
            user_x, reply_x,
            "the user's message and the agent's reply share the pane's left edge"
        );

        // The user's own message: the square band behind its text, running from
        // the pane's left edge to its right.
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
            .expect("the user's message paints its band");
        assert_eq!(band.1, 0.0, "the user's band has square corners");
        assert_eq!(
            band.0.min_x(),
            crate::ui::SIDEBAR_WIDTH,
            "the user's band starts at the pane's left edge"
        );
        assert_eq!(
            band.0.max_x(),
            1024.0,
            "the user's band runs to the pane's right edge"
        );
        let padded = user_x - band.0.min_x();
        assert!(
            (10.0..=15.0).contains(&padded),
            "the transcript keeps 10-15 px of padding, got {padded}"
        );
    }
}

#[cfg(test)]
mod link_action_tests {
    use super::*;
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

    /// Build the pane's transcript, lay it out, and click the run `label`.
    fn click_message(
        message: ChatMessage,
        on_open_url: Rc<RefCell<dyn FnMut(String)>>,
        label: &str,
    ) {
        let ignored: Rc<RefCell<dyn FnMut(String)>> = Rc::new(RefCell::new(|_| {}));
        click_transcript(message, Default::default(), on_open_url, ignored, label)
    }

    /// The same click through a transcript that holds `sub_agents` for the pane
    /// and routes a sub-agent row's action to `on_open_sub_agent`.
    fn click_transcript(
        message: ChatMessage,
        sub_agents: std::collections::HashMap<String, goble_ui::SubAgentRow>,
        on_open_url: Rc<RefCell<dyn FnMut(String)>>,
        on_open_sub_agent: Rc<RefCell<dyn FnMut(String)>>,
        label: &str,
    ) {
        let app = AppContext::default();
        let mut view = ChatView::new()
            .with_messages(vec![message])
            .with_sub_agents(sub_agents)
            .with_on_action(chat_action_relay(on_open_url, on_open_sub_agent));
        view.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
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
                RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the transcript must draw the link run {label:?}"));
        let position = origin + vec2f(2.0, 4.0);
        let mut ctx = EventContext::default();
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

    /// S5: the sub-agent row's click terminates in the app's child opener,
    /// carrying the child's conversation id. The transcript reads the row from
    /// the pane's live records, so the label it clicks is the record's own line.
    #[test]
    fn a_sub_agent_row_click_reaches_the_app_child_opener() {
        let opened = Rc::new(RefCell::new(Vec::new()));
        let recorder = opened.clone();
        let call = ToolCall {
            id: "call_spawn".to_string(),
            name: "spawn_subagent".to_string(),
            arguments: r#"{"subagent_type":"reviewer","input":"audit the migration"}"#.to_string(),
            status: goble_core::harness::ToolCallStatus::Running,
            result: None,
        };
        let message =
            ChatMessage::new(ChatRole::Assistant, vec![]).with_tool_calls(vec![call.clone()]);
        let record = SubAgentRow {
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
        };
        let ignored: Rc<RefCell<dyn FnMut(String)>> = Rc::new(RefCell::new(|_| {}));
        let opener: Rc<RefCell<dyn FnMut(String)>> = Rc::new(RefCell::new(move |child_id| {
            recorder.borrow_mut().push(child_id)
        }));

        click_transcript(
            message,
            HashMap::from([("call_spawn".to_string(), record)]),
            ignored,
            opener,
            "open child",
        );
        assert_eq!(
            *opened.borrow(),
            vec!["conv-child-1".to_string()],
            "the row's click must reach the app carrying the child's conversation id"
        );
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
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::AppContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::{pill_rects, rect_contains, render_element};
    use goble_core::harness::ToolCallStatus;
    use goble_ui::{SubAgentRow, SubAgentRowStatus, ToolCall};
    use std::sync::Arc;
    use std::time::Duration;

    /// The pane's own run `label`, as drawn: a run left of the sidebar is the
    /// sidebar quoting the conversation, not the transcript.
    fn pane_run_origin(commands: &[RenderCommand], label: &str) -> Option<goble_ui::Vector2F> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if text == label && origin.x >= crate::ui::SIDEBAR_WIDTH =>
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
        };
        let edit = ToolCall {
            id: "call_edit".to_string(),
            name: "edit_file".to_string(),
            arguments: r#"{"path":"src/panes.rs","old_text":"let a = 1;","new_text":"let a = 2;"}"#
                .to_string(),
            status: ToolCallStatus::Error,
            result: Some("old_text not found".to_string()),
        };
        let search = ToolCall {
            id: "call_search".to_string(),
            name: "codebase_search".to_string(),
            arguments: r#"{"pattern":"fn build","path":"src"}"#.to_string(),
            status: ToolCallStatus::Running,
            result: None,
        };
        let spawn = ToolCall {
            id: "call_spawn".to_string(),
            name: "spawn_subagent".to_string(),
            arguments: r#"{"subagent_type":"reviewer","input":"audit the migration"}"#.to_string(),
            status: ToolCallStatus::Running,
            result: None,
        };

        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.active_pane_id = 1;
            let mut runtime = PaneRuntime::default();
            runtime.busy = true;
            runtime.messages = vec![
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
            ];
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
            s.pane_runtime.insert(1, runtime);
        }

        let mut root: Box<dyn Element> = Box::new(view);
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);

        // The rows the transcript drew, as the runs they own. A pill is a
        // rounded box (or a border) drawn over the row its run belongs to.
        let row_labels = [
            "Fix the build",
            "Reading the sidebar, then the panes.",
            "src/sidebar.rs",
            "src/panes.rs",
            "\"fn build\" in src",
            "Subagent ",
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

        // The pane's own split frame encloses every row: it is window chrome
        // (a square border around the pane), not a row's box. Every other pill
        // is the pane's to explain.
        let pills: Vec<goble_ui::RectF> = pill_rects(&commands)
            .into_iter()
            .filter(|pill| !rows.iter().all(|row| rect_contains(*pill, *row)))
            .collect();
        let frame = pill_rects(&commands)
            .into_iter()
            .find(|pill| rows.iter().all(|row| rect_contains(*pill, *row)));
        if let Some(frame) = frame {
            // The frame is square: a rounded pane card would be chrome of its
            // own, and the transcript rows must not sit inside one.
            let filled_round = commands.iter().any(|command| {
                matches!(command, RenderCommand::FillRect { rect, corner_radius, .. }
                    if *rect == frame && *corner_radius > 0.0)
            });
            assert!(!filled_round, "the pane's own frame is square, got {frame:?}");
        }

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
