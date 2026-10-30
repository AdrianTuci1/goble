
use std::cell::RefCell;
use std::rc::Rc;

use goble_core::harness::CommandDecision;
use goble_ui::elements::{
    slash_menu_open, AppContext, CrossAxisAlignment, Element, Flex, ShortcutHint, SlashMenu,
};
use goble_ui::ChatComposer;

use crate::state::PaneControls;
use crate::terminal::{classify_input, InputClass};
use crate::ui::palette::slash_accept;

use super::super::{UiActions, UiSnapshot};
use super::pane_session_snapshot;

/// The rich input a terminal pane pins to the bottom of its pty.
///
/// It is the very [`ChatComposer`] the chat surface draws — one widget, the same
/// controls, the same send affordance (Enter submits), in the same place at the
/// bottom — wired to this pane's own session instead of a transcript's, the way
/// `ChatView`'s composer is wired for a chat pane.
///
/// Enter runs the draft as a shell command in this pane's own PTY — the same
/// thing the grid above the bar does with a line typed there, and the shell side
/// of the model the chat composer carries, where a `!`-prefixed draft is a
/// command. Cmd/Ctrl+Enter opens the pane's harness and starts a new
/// conversation; from there the agent view's composer takes prompts.
///
/// `focused` is the pane's decision, not the composer's: the shell pane's rich
/// input is the only place a command can be typed, so it holds the keyboard
/// whenever the pane is active and no full-screen program owns the screen.
///
/// The agent's own environment controls are left out deliberately (the harness
/// pill, the model selector and the attach button), because they describe a
/// turn this bar never runs. What it keeps above the editor is the shell's own
/// context: the working directory and the branch.
///
/// A command the harness suspended on is a card at the end of the pane's
/// output, in the shell view as in the agent view: leaving the agent view while
/// the turn is suspended keeps the decision in front of the user, and this
/// bar's own keys decide the same proposal.
pub(crate) fn build_terminal_composer(
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
    focused: bool,
) -> Box<dyn Element> {
    let session = pane_session_snapshot(state, pane_id);
    let controls = state.pane_controls.get(&pane_id).cloned().unwrap_or_else(|| {
        PaneControls::new(state.selected_model.clone(), state.auto_approve, String::new())
    });
    // Where this pane's shell is: the host a submitted `ssh` line bound it to
    // (S1), read from the session the binding lives in rather than from a copy.
    // A pane at a local shell has no entry, so its bar draws no chip.
    let ssh_session = state.terminal.borrow().ssh_session(pane_id).cloned();

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_stop = actions.on_stop.clone();
    let on_command_decision = actions.on_command_decision.clone();
    let on_slash_move = actions.on_slash_move.clone();
    let on_slash_close = actions.on_slash_close.clone();
    let on_slash_dismiss = actions.on_slash_dismiss.clone();
    // The same two gestures drive the list itself, which this bar draws above
    // the composer rather than inside it.
    let on_slash_move_menu = on_slash_move.clone();
    let on_slash_close_menu = on_slash_close.clone();

    let on_activate_for_send = actions.on_pane_activate.clone();
    let on_run_shell_command = actions.on_run_shell_command.clone();
    let on_activate_for_cmd = actions.on_pane_activate.clone();
    let on_harness_mode_for_cmd = actions.on_set_pane_harness_mode.clone();
    let on_cmd_enter = actions.on_cmd_enter.clone();
    let on_activate_for_focus = actions.on_pane_activate.clone();

    let mut composer = ChatComposer::new()
        .with_value(session.composer_draft.clone())
        // The bar owns the pane's keyboard while the pane is active and the
        // screen still belongs to the shell; a background pane's input never
        // swallows the keys the active pane needs.
        .with_focused(focused)
        // A suspended command is a card over this bar: until the user decides,
        // the bar is not a typing surface, so typing cannot go to the shell
        // while the harness waits.
        .with_disabled(session.pending_command.is_some())
        // The rich input is the shell pane's only typing surface: a click on
        // the output above it must not blur the editor.
        .with_blur_on_outside_click(false)
        .with_caret(controls.caret.clone())
        .with_path_label(crate::state::display_path(&session.composer_path))
        // Where the pane's shell is, when the `ssh` line the user submitted
        // bound it to a host: the chip over the editor names it, and a local
        // shell draws no chip at all.
        .with_ssh_session(ssh_session)
        .with_stop_visible(session.agent_busy)
        // The shell's own context — this pane's working directory and its git
        // branch — rides above the editor: it describes the command line below
        // it, the way the shell prompt does.
        .with_context_above_editor(true)
        // The bar is the pane's own bottom surface, so it spans the pane: no
        // gutter of its own between the input and the pane's side edges.
        .with_edge_to_edge(true)
        // What the bar answers, written where the bar is: Cmd/Ctrl+Enter is the
        // one gesture that leaves the shell (it opens the agent on a new
        // conversation), so it is the one instruction the strip names — and it
        // is the one entry this pane keeps inside the input, under the editor,
        // rather than over the separator with the agent view's strip.
        .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")])
        .with_on_change(move |text| (on_composer_change.borrow_mut())(text))
        .with_on_send(move |text| {
            // Enter at the shell's bar runs the draft as a command in this
            // pane's own PTY — the shell's side of the warp-new input model,
            // where the agent composer's `!cmd` is the same thing. The harness
            // is not opened by it: Cmd/Ctrl+Enter is what opens the agent view,
            // and the agent view's composer is the one that takes prompts.
            (on_activate_for_send.borrow_mut())(pane_id);
            if let Some(InputClass::TerminalCommand(command)) = classify_input(&text, false) {
                (on_run_shell_command.borrow_mut())(pane_id, command);
            }
        })
        .with_on_cmd_enter(move |text| {
            // Cmd/Ctrl+Enter starts a new conversation here, the same gesture
            // the chat pane's composer carries.
            (on_activate_for_cmd.borrow_mut())(pane_id);
            (on_harness_mode_for_cmd.borrow_mut())(pane_id, true);
            (on_cmd_enter.borrow_mut())(text);
        })
        .with_on_focus_change(move |focused| {
            if focused {
                (on_activate_for_focus.borrow_mut())(pane_id);
            }
            (on_composer_focus.borrow_mut())(focused);
        })
        .with_on_stop(move || (on_stop.borrow_mut())());
    // A suspended command is the bar while it lasts: the same approval
    // surface the agent view draws, answering the same decision handler.
    if let Some(proposal) = session.pending_command.clone() {
        composer = composer.with_proposal(proposal);
    }
    composer = composer
        .with_proposal_selection(session.command_selection.clone())
        .with_on_decision(move |id, decision| {
            (on_command_decision.borrow_mut())(pane_id, id, decision)
        });

    // The bar's own slash commands: `/` opens the palette's command list above
    // the editor, narrowed by what was typed after the slash. Same list as the
    // Cmd+K overlay, drawn in place.
    let slash_commands = crate::ui::palette::slash_commands(state, actions);
    let slash_query =
        crate::ui::palette::slash_query(&session.composer_draft).unwrap_or_default();
    let slash_filtered = crate::ui::palette::matching_slash_commands(&slash_commands, &slash_query);
    let slash_items = crate::ui::palette::slash_items(&slash_filtered);
    composer = composer
        .with_slash_menu(
            slash_items.clone(),
            controls.slash_index.clone(),
            controls.slash_dismissed.clone(),
        )
        .with_on_slash_move(move |index| (on_slash_move.borrow_mut())(index))
        .with_on_slash_accept(crate::ui::palette::slash_accept(
            slash_filtered.clone(),
            on_slash_close,
        ))
        .with_on_slash_dismiss(move || (on_slash_dismiss.borrow_mut())());

    // Modal editing is the user's choice, and the mode state is this pane's own
    // (see `PaneControls::vim`). The clipboard is the app's system-clipboard
    // handle, so `"+y` reaches outside the process.
    if state.vim_mode {
        composer = composer
            .with_vim(controls.vim.clone())
            .with_clipboard(actions.clipboard.clone());
    }

    // warp-new context pills, the shell's own two: this pane's working
    // directory and its git branch, each routing to this pane's controls. There
    // is no harness pill and no model here — a shell command runs on neither;
    // both belong to the agent view this bar opens with Cmd/Ctrl+Enter, which
    // shows the harness at the top and the model under its editor.
    if let Some(context) = state.composer_context.get(&pane_id) {
        let dir_ids = context.dir_ids.clone();
        let on_select_dir = actions.on_select_dir.clone();
        composer = composer.with_dir_menu(
            context.dir_items.clone(),
            context.dir_menu_open.clone(),
            move |idx| {
                if let Some(id) = dir_ids.get(idx) {
                    (on_select_dir.borrow_mut())(pane_id, id.clone());
                }
            },
        );
        composer = composer.with_dir_menu_scroll(context.dir_menu_scroll.clone());
        if !context.branch_label.is_empty() {
            let branch_ids = context.branch_ids.clone();
            let on_select_branch = actions.on_select_branch.clone();
            composer = composer
                .with_branch_label(context.branch_label.clone())
                .with_branch_menu(
                    context.branch_items.clone(),
                    context.branch_menu_open.clone(),
                    move |idx| {
                        if let Some(id) = branch_ids.get(idx) {
                            (on_select_branch.borrow_mut())(pane_id, id.clone());
                        }
                    },
                );
        }
        if let Some(diff) = context.diff.clone() {
            let open = actions.on_diff_panel.clone();
            composer = composer
                .with_diff_menu(diff, context.diff_menu_open.clone())
                .with_diff_menu_scroll(context.diff_menu_scroll.clone())
                .with_diff_open(move |file| {
                    (open.borrow_mut())(pane_id, crate::ui::DiffPanelAction::Open(file))
                });
        }
    }

    // The command list the draft is typing is drawn over the whole bar, above
    // it — over the instruction strip the composer carries at its bottom — the
    // way grok-build puts it over the prompt.
    let composer = composer.finish();
    if session.pending_command.is_some()
        || !slash_menu_open(&session.composer_draft, *controls.slash_dismissed.borrow())
    {
        return composer;
    }
    let menu = SlashMenu::new(slash_items, controls.slash_index.clone())
        .with_on_move(move |index| (on_slash_move_menu.borrow_mut())(index))
        .with_on_accept(slash_accept(slash_filtered, on_slash_close_menu));
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(menu.finish())
        .with_child(composer)
        .finish()
}
/// The approval card for the command this pane's harness is suspended on, with
/// the height the pane reserves for it so its pty keeps the rows it can see.
/// The card closes the shell's output exactly where the command would run, so a
/// turn suspended behind it stays visible after the user leaves the agent view
/// for the shell.
pub(crate) struct PendingApproval {
    pub element: Box<dyn Element>,
    pub height: f32,
}

pub(crate) fn build_pending_approval(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
) -> Option<PendingApproval> {
    use goble_ui::theme::SpacingToken;

    let session = pane_session_snapshot(state, pane_id);
    let proposal = session.pending_command.as_ref()?;
    let on_command_decision = actions.on_command_decision.clone();
    let handler: goble_ui::elements::DecisionHandler =
        Rc::new(RefCell::new(move |id: String, decision: CommandDecision| {
            (on_command_decision.borrow_mut())(pane_id, id, decision);
        }));
    let element = goble_ui::elements::command_approval_card(
        proposal,
        &goble_ui::elements::CommandApprovalState {
            selection: session.command_selection.clone(),
            scroll: session.command_scroll.clone(),
            editor_scroll: session.command_editor_scroll.clone(),
            draft: session.command_draft.clone(),
            caret: session.command_caret.clone(),
            // A terminal pane has no follow-up input of its own while the
            // command waits, so the card always holds the keyboard.
            field_focused: true,
        },
        Some(handler),
        app,
    );
    // The header band (question and the two controls), the separator, the
    // candidate list when there is one, the editable command flush with the
    // card's edges, and the padded hint under it. The editor grows with the
    // command's lines up to its cap.
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let variants = match proposal.candidates.len() {
        0 | 1 => 0.0,
        count => (count as f32 * 24.0 + 4.0)
            .clamp(24.0, goble_ui::elements::APPROVAL_BODY_MAX_HEIGHT),
    };
    let lines = session.command_draft.borrow().matches('\n').count() + 1;
    let editor = (lines as f32 * 16.8 + 2.0)
        .clamp(28.0, goble_ui::elements::APPROVAL_EDITOR_MAX_HEIGHT);
    let mut height = md + sm + 28.0 + 1.0 + editor + sm + 12.0 + sm;
    if variants > 0.0 {
        height += variants + sm;
    }
    Some(PendingApproval { element, height })
}
