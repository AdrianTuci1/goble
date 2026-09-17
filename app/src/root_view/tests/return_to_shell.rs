//! U1: a pane that goes back to the shell really goes back.
//!
//! A terminal pane draws one of two surfaces, and one flag decides which:
//! `PaneControls::harness_mode`, read through `PaneControls::surface` — the same
//! reading `UiState::pane_view` hands every caller and `build_terminal` paints
//! from. The state that used to disagree was reachable: an agent-mode terminal
//! pane with a sub-agent child view open, then a sidebar conversation switch or
//! an agent delete (both call `UiState::bind_active_pane_conversation`), which
//! put the pane's view back on the terminal while the harness stayed on — so the
//! pane went on painting the agent surface while the root's Cmd/Ctrl+F chord
//! read the pane as a shell and never mounted the shell's handler, and a `!cmd`
//! typed into the surface on screen carried no conversation.
//!
//! Each case drives the app's own gesture, then asserts the three things a user
//! sees: the pane paints the shell, the filter chord is answered (by the shell
//! pane's own handler, which the root leaves the chord to), and a `!cmd` filed
//! from the pane's agent surface runs in the conversation's own block. The
//! pixels are not seen — no window is opened — so the surface is read from the
//! frame the app draws and the chord from the state the dispatched key leaves.

use super::*;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_terminal::blocks::{BlockId, BlockView};
use goble_terminal::hooks::{encode_hook, HookEvent};
use goble_ui::event::ModifiersState;
use goble_ui::geometry::vec2f;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;

use crate::emulator::Emulator;
use crate::state::UiState;
use crate::terminal::TerminalSession;
use crate::ui::{Pane, PaneKind, Space};

/// The pane both U1 cases start from.
struct Fixture {
    root: Box<dyn Element>,
    state: Rc<RefCell<UiState>>,
    /// The pane's own conversation.
    parent: String,
    /// The sub-agent child conversation the pane's second view shows.
    child: String,
    /// The conversation the sidebar switch selects.
    other: String,
    select: Rc<RefCell<dyn FnMut(String)>>,
    delete: Rc<RefCell<dyn FnMut(String)>>,
    harness: Rc<RefCell<dyn FnMut(u64, bool)>>,
    send: Rc<RefCell<dyn FnMut(String)>>,
    _dir: tempfile::TempDir,
}

/// An agent-mode terminal pane with a sub-agent child view open, in a window
/// that draws it: Cmd+Enter's switch is on, the child's view is what the pane is
/// filtered to, and the pane's shell is at a prompt (`Bootstrapped`), so a block
/// is waiting for the next command.
fn agent_pane_over_a_child_view() -> Fixture {
    let app = AppContext::default();
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let parent = desktop
        .create_chat("Parent conversation", None, None)
        .expect("parent chat");
    let other = desktop
        .create_chat("Another conversation", None, None)
        .expect("another chat");
    let child = "conv-child-1".to_string();

    let root = RootView::new(&app, &desktop, None);
    let state = root.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        // One terminal pane, holding the conversation the harness was opened on.
        s.spaces = vec![Space::new(
            "Shell",
            Pane::Leaf {
                id: 1,
                kind: PaneKind::Terminal,
            },
        )];
        s.active_space = 0;
        s.active_pane_id = 1;
        s.selected_id = Some(parent.clone());
        s.pane_sessions
            .get_mut(&1)
            .expect("the default pane session exists")
            .conversation_id = parent.clone();
        s.ensure_pane_controls();
        // A shell that has bootstrapped its integration: the block the next
        // command runs in is at a prompt, which is the block a `!cmd` claims.
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(&encode_hook(&HookEvent::Bootstrapped(Default::default())));
        s.terminal
            .borrow_mut()
            .sessions
            .insert(1, TerminalSession::with_emulator(emulator));
    }

    // The app's own callbacks, as the mounted root built them.
    let (select, delete, harness, send) = {
        let actions = root.actions.as_ref().expect("the root built its actions");
        (
            Rc::clone(&actions.on_select_conversation),
            Rc::clone(&actions.on_agent_delete),
            Rc::clone(&actions.on_set_pane_harness_mode),
            Rc::clone(&actions.on_send_message),
        )
    };

    // Cmd+Enter on the pane, and the sub-agent row's click inside the view it
    // opened: the two gestures whose combination is the state this item fixes.
    (harness.borrow_mut())(1, true);
    state.borrow_mut().open_sub_agent(1, &child, None);

    Fixture {
        root: Box::new(root),
        state,
        parent,
        child,
        other,
        select,
        delete,
        harness,
        send,
        _dir: dir,
    }
}

/// The drawn runs of the pane's own area: a run left of the sidebar is the
/// sidebar, not the pane.
fn pane_runs(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<String> {
    render_element(root, vec2f(1024.0, 768.0), app)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

fn pane_has(runs: &[String], text: &str) -> bool {
    runs.iter().any(|run| run == text)
}

/// The conversation's own blocks, drawn in the pane's terminal surface.
fn terminal_block_ids(state: &Rc<RefCell<UiState>>, pane_id: u64) -> Vec<BlockId> {
    state
        .borrow()
        .pane_terminal_view(pane_id)
        .into_iter()
        .map(|block| block.id)
        .collect()
}

impl Fixture {
    /// Run `gesture` — the conversation switch, or the agent delete — and then
    /// hold the pane to the three things "back at the shell" means.
    fn return_to_shell(mut self, gesture: impl FnOnce(&Self)) {
        let app = AppContext::default();
        let pane_id = 1u64;

        // The premise: the pane is in agent mode over the child's conversation,
        // and the frame draws that surface — a title only the child view draws.
        assert!(
            self.state.borrow().pane_controls(pane_id).harness_mode,
            "the pane is in agent mode"
        );
        assert_eq!(
            self.state.borrow().pane_view(pane_id),
            BlockView::Agent {
                conversation_id: self.child.clone()
            },
            "the pane's agent surface is filtered to the child's conversation"
        );
        let runs = pane_runs(&mut self.root, &app);
        assert!(
            pane_has(&runs, "Esc to return"),
            "the agent surface is what the pane draws before the switch: {runs:?}"
        );
        assert!(
            !pane_has(&runs, "new conversation"),
            "and it is not the shell: {runs:?}"
        );

        gesture(&self);

        // 1. The pane draws the shell surface, and the one flag that decides the
        // surface says so as well.
        {
            let state = self.state.borrow();
            assert_eq!(
                state.pane_view(pane_id),
                BlockView::Terminal,
                "the pane is back on the shell"
            );
            assert!(
                !state.pane_controls(pane_id).harness_mode,
                "and the harness switch went off with it, so the surface drawn and \
                 the surface read are the same one"
            );
            assert!(
                state.sub_agent_view(pane_id).is_none(),
                "the child view does not follow the pane into another conversation"
            );
        }
        let runs = pane_runs(&mut self.root, &app);
        assert!(
            pane_has(&runs, "new conversation"),
            "the shell's own rich input is what the pane draws now: {runs:?}"
        );
        assert!(
            !pane_has(&runs, "send to cloud") && !pane_has(&runs, "Esc to return"),
            "and no agent surface is left over it: {runs:?}"
        );

        // 2. Cmd/Ctrl+F is handled. The root leaves the chord to a shell pane,
        // and the shell surface is now the one mounted, so the pane's own
        // handler opens its whole-output filter.
        let cmd_f = DispatchedEvent::KeyDown {
            key: "f".to_string(),
            modifiers: ModifiersState {
                command: true,
                ..ModifiersState::default()
            },
        };
        assert!(
            self.root.dispatch_event(&cmd_f, &mut EventContext, &app),
            "the pane's own filter chord answers Cmd+F"
        );
        let filter = self
            .state
            .borrow()
            .terminal_global_filters
            .get(&pane_id)
            .cloned()
            .expect("the pane's own filter is seeded");
        assert!(filter.is_open(), "Cmd+F raised the shell pane's filter bar");
        assert!(
            *filter.focused.borrow(),
            "and put the caret in its field, which is what types into it"
        );

        // 3. A `!cmd` typed into the pane's agent surface carries the pane's
        // conversation: Cmd+Enter reopens that view, and the block the command
        // runs in becomes the conversation's own — the shell's history leaves it
        // out, which is the whole of what an owner does to a block.
        let conversation = self
            .state
            .borrow()
            .pane_conversation_id(pane_id)
            .expect("the pane kept a conversation of its own");
        (self.harness.borrow_mut())(pane_id, true);
        assert_eq!(
            self.state.borrow().pane_view(pane_id),
            BlockView::Agent {
                conversation_id: conversation.clone()
            },
            "Cmd+Enter opens the agent view of the conversation the pane is on"
        );
        let before = terminal_block_ids(&self.state, pane_id);
        (self.send.borrow_mut())("!ls".to_string());
        let after = terminal_block_ids(&self.state, pane_id);
        let claimed: Vec<BlockId> = before
            .into_iter()
            .filter(|id| !after.contains(id))
            .collect();
        assert_eq!(
            claimed.len(),
            1,
            "exactly one shell block became the conversation's"
        );
        assert!(
            self.state
                .borrow()
                .pane_agent_view(pane_id, &conversation)
                .iter()
                .any(|block| block.id == claimed[0]),
            "the `!cmd` runs in the conversation's own block"
        );
    }
}

/// The sidebar switch: selecting another conversation binds the active pane to
/// it, which is the caller whose own comment says the pane goes back to the
/// shell.
#[test]
fn a_conversation_switch_that_returns_a_pane_to_the_shell_really_returns_it() {
    let fixture = agent_pane_over_a_child_view();
    let other = fixture.other.clone();
    fixture.return_to_shell(move |fixture| (fixture.select.borrow_mut())(other));
}

/// The agent delete: removing the conversation the pane is on rebinds it to the
/// next one through the same binder.
#[test]
fn an_agent_delete_that_returns_a_pane_to_the_shell_really_returns_it() {
    let fixture = agent_pane_over_a_child_view();
    let parent = fixture.parent.clone();
    fixture.return_to_shell(move |fixture| (fixture.delete.borrow_mut())(parent));
}
