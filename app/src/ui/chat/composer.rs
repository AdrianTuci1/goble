
use goble_ui::elements::Element;
use goble_ui::ChatComposer;

use crate::state::PaneControls;
use crate::terminal::{classify_input, InputClass};

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
/// Two controls are left out deliberately: the command-proposal card — a
/// suspended command only exists while the harness owns a pane's input, and a
/// pane whose harness is open draws the shared agent view, that card included,
/// instead of this bar — and the agent's own environment controls (the harness
/// pill, the model selector and the attach button), which describe a turn this
/// bar never runs. What it keeps above the editor is the shell's own context:
/// the working directory and the branch.
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

    let on_composer_change = actions.on_composer_change.clone();
    let on_composer_focus = actions.on_composer_focus_change.clone();
    let on_stop = actions.on_stop.clone();
    let on_composer_slash = actions.on_composer_slash.clone();

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
        // The rich input is the shell pane's only typing surface: a click on
        // the output above it must not blur the editor.
        .with_blur_on_outside_click(false)
        .with_caret(controls.caret.clone())
        .with_path_label(crate::state::display_path(&session.composer_path))
        .with_stop_visible(session.agent_busy)
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
        .with_on_stop(move || (on_stop.borrow_mut())())
        .with_on_slash(move || (on_composer_slash.borrow_mut())());

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
    }

    composer.finish()
}
