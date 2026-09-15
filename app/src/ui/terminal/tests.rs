use super::blocks::block_status;
use super::cards::build_card;
use crate::emulator::{AgentViewCard, VisibleBlock};
use crate::terminal::{TerminalRegistry, TerminalSession};
use goble_ui::elements::{
    AppContext, EventContext, TerminalBlockPlumbing, TerminalData, TerminalGrid, TerminalStatus,
    Text,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::theme::FontFamily;
use std::collections::HashMap;

    use super::*;
    use crate::emulator::Emulator;
    use crate::state::UiState;
    use goble_core::harness::ToolCallStatus;
    use goble_ui::elements::{
        terminal_block, ChatFragment, ChatMessageBubble, ChatRole, TerminalFilter, ToolCall,
        CARD_RAIL_WIDTH,
    };
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::{ChatComposer, ColorU};

    fn key(key: &str, modifiers: ModifiersState) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: key.to_string(),
            modifiers,
        }
    }

    /// In a plain pty pane Cmd+Enter turns the harness on for this pane only.
    #[test]
    fn cmd_enter_activates_harness_mode_on_a_shell_pane() {
        let app = AppContext::default();
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        terminal
            .borrow_mut()
            .set_input(3, "summarize the diff".to_string());
        let routed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let harness: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let routed_cb = Rc::clone(&routed);
        let harness_cb = Rc::clone(&harness);
        let mut view = TerminalView::new(
            Rc::clone(&terminal),
            3,
            "/tmp".to_string(),
            true,
            BlockView::Terminal,
            false,
            Text::new("bar").finish(),
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(move |_: u64, text: String| {
                routed_cb.borrow_mut().push(text)
            })),
            Rc::new(RefCell::new(move |_: u64, on: bool| {
                harness_cb.borrow_mut().push(on)
            })),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
        );
        let mut ctx = EventContext::default();
        let cmd_enter = ModifiersState {
            command: true,
            ..ModifiersState::default()
        };
        assert!(view.dispatch_event(&key("Enter", cmd_enter), &mut ctx, &app));
        assert_eq!(*harness.borrow(), vec![true], "Cmd+Enter activates it");
        assert_eq!(*routed.borrow(), vec!["summarize the diff".to_string()]);
    }

    /// A background pane must not take keystrokes from the active one.
    #[test]
    fn an_inactive_pane_ignores_keystrokes() {
        let app = AppContext::default();
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        let mut view = TerminalView::new(
            terminal,
            1,
            "/tmp".to_string(),
            false,
            BlockView::Terminal,
            false,
            Text::new("bar").finish(),
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
            Rc::new(RefCell::new(|_: u64, _: bool| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
        );
        let mut ctx = EventContext::default();
        assert!(!view.dispatch_event(&key("a", ModifiersState::none()), &mut ctx, &app));
    }

    /// The pointer becomes a cell against the grid that was painted, not
    /// against a second guess at the flex layout.
    #[test]
    fn a_pointer_position_maps_to_the_cell_it_is_over() {
        let geometry = GridGeometry {
            origin: Vector2F::new(10.0, 20.0),
            columns: 80,
            rows: 24,
        };
        let pitch = TerminalGrid::cell_width(FONT_SIZE);
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT);

        assert_eq!(geometry.cell_at(Vector2F::new(10.0, 20.0)), Some((0, 0)));
        assert_eq!(
            geometry.cell_at(Vector2F::new(10.0 + pitch * 3.5, 20.0 + row_pitch * 4.5)),
            Some((4, 3))
        );
        // Outside the box, on either side, is not a cell.
        assert_eq!(geometry.cell_at(Vector2F::new(9.0, 20.0)), None);
        assert_eq!(geometry.cell_at(Vector2F::new(10.0, 19.0)), None);
        assert_eq!(
            geometry.cell_at(Vector2F::new(10.0 + pitch * 81.0, 20.0)),
            None
        );
        assert_eq!(
            geometry.cell_at(Vector2F::new(10.0, 20.0 + row_pitch * 25.0)),
            None
        );
    }

    #[test]
    fn mouse_button_ids_map_to_the_buttons_the_platform_numbers() {
        assert_eq!(mouse_button(0), Some(MouseButton::Left));
        assert_eq!(mouse_button(1), Some(MouseButton::Right));
        assert_eq!(mouse_button(2), Some(MouseButton::Middle));
        assert_eq!(mouse_button(7), None);
    }

    /// Focus is reported to the program only when the pane is the active one and
    /// only after the program asked for it (mode 1004).
    #[test]
    fn only_an_active_pane_with_the_mode_on_reports_focus() {
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        let view = |active: bool| {
            TerminalView::new(
                Rc::clone(&terminal),
                1,
                "/tmp".to_string(),
                active,
                BlockView::Terminal,
                false,
                Text::new("bar").finish(),
                Rc::new(RefCell::new(|_: u64| {})),
                Rc::new(RefCell::new(|_: u64, _: String| {})),
                Rc::new(RefCell::new(|_: u64, _: bool| {})),
                Rc::new(RefCell::new(|_: u64, _: String| {})),
            )
        };

        // No session, so the mode is unknown and the encoder declines either way.
        assert!(!view(true).report_focus(true), "1004 is not on");
        assert!(
            !view(false).report_focus(true),
            "a background pane is not the focused one"
        );
    }

    /// The drawn text runs as `(text, colour, family, size)`, so two elements
    /// can be compared without depending on where each was laid out.
    fn text_runs(commands: &[RenderCommand]) -> Vec<(String, ColorU, FontFamily, f32)> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText {
                    text,
                    color,
                    font_family,
                    font_size,
                    ..
                } => Some((text.clone(), *color, *font_family, *font_size)),
                _ => None,
            })
            .collect()
    }

    /// Paint an element headlessly at a fixed size.
    fn paint(element: Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        let mut element = element;
        render_element(&mut element, vec2f(600.0, 200.0), app)
    }

    /// Q8: a command is one block, drawn by the same element in the transcript
    /// and in the terminal pane. The pane's executed-command block renders
    /// identically in the transcript, and a command the agent ran is that same
    /// block built from the command the bridge produced for it.
    #[test]
    fn a_command_block_renders_identically_in_the_transcript_and_the_pane() {
        let app = AppContext::default();

        // The pane's block, from a session that ran `echo hi`.
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(b"echo hi\r\nhi\r\n");
        let session = TerminalSession::with_emulator(emulator);
        let pane_data = executed_command_block(&session).expect("the pane drew its block");
        let pane_runs = text_runs(&paint(
            terminal_block(&pane_data, TerminalFilter::default(), None, None),
            &app,
        ));
        assert!(
            !pane_runs.is_empty(),
            "the pane's block draws the session's output"
        );

        // The transcript draws the pane's block as the same element.
        let transcript = ChatMessageBubble::new(
            ChatRole::Tool,
            vec![ChatFragment::terminal(pane_data.clone())],
        );
        let transcript_runs = text_runs(&paint(Box::new(transcript), &app));
        assert_eq!(
            transcript_runs, pane_runs,
            "the same block must render in the transcript"
        );

        // A command the agent ran is that block, built from the command and its
        // output rather than re-drawn as agent output.
        let command = TerminalData::for_command("echo hi", "hi", TerminalStatus::Success);
        let command_runs = text_runs(&paint(
            terminal_block(&command, TerminalFilter::default(), None, None),
            &app,
        ));
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "run_command".to_string(),
            arguments: r#"{"command":"echo hi"}"#.to_string(),
            status: ToolCallStatus::Finished,
            result: Some("hi".to_string()),
        };
        // The command starts folded; expand it so the transcript draws the same
        // block the pane does.
        let fold = Rc::new(RefCell::new(std::collections::HashMap::from([(
            goble_ui::tool_fold_key(&call, 0),
            goble_ui::ToolDisplayMode::Expanded,
        )])));
        let bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
            .with_tool_calls(vec![call])
            .with_tool_fold(fold);
        let bubble_runs = text_runs(&paint(Box::new(bubble), &app));
        // The header is the mark, the verb and the command it runs for; the
        // block itself follows it unchanged.
        assert_eq!(
            &bubble_runs[3..],
            command_runs.as_slice(),
            "the agent's command segment must be the terminal block"
        );
    }

    /// R5: a command block is drawn from the block's lifecycle, never from a
    /// guess. `block_status` is the one status map the pane's executed-command
    /// block uses, and it feeds the one `terminal_block` the transcript draws,
    /// so a still-running command is painted as running and a failure is the
    /// exit code. (This replaces the assertion that used to run through the
    /// deleted pty-only agent view `TerminalView::build_agent_view`; the
    /// behaviour it encoded is the same, on the surface that survives. It read
    /// the status off the block's title colour, which warp's failing block does
    /// not use: the status is now the block's own pole — accent while running,
    /// error on failure — and the failure's wash, so that is what it asserts.)
    #[test]
    fn a_command_block_is_drawn_from_its_lifecycle_not_a_guess() {
        use goble_terminal::{BlockId, BlockOwner, BlockState};
        use goble_ui::theme::ColorToken;

        let app = AppContext::default();
        let block = |command: &str, state: BlockState, exit_code: Option<i32>| VisibleBlock {
            id: BlockId(1),
            owner: BlockOwner::Agent {
                conversation_id: "c1".to_string(),
                call_id: "call-1".to_string(),
            },
            command: command.to_string(),
            output: String::new(),
            state,
            exit_code,
            pwd: None,
            git_branch: None,
            duration: None,
            card: None,
        };

        let running = block("sleep 30", BlockState::Executing, None);
        let background = block("tail -f log", BlockState::Background, None);
        let done = block("ls", BlockState::DoneWithExecution, Some(0));
        let failed = block("boom", BlockState::DoneWithExecution, Some(2));
        assert_eq!(block_status(&running), TerminalStatus::Running);
        assert_eq!(block_status(&background), TerminalStatus::Running);
        assert_eq!(block_status(&done), TerminalStatus::Success);
        assert_eq!(block_status(&failed), TerminalStatus::Error);

        // The drawn block, built the way the transcript builds it: the block's
        // own status, through the one shared renderer.
        let marks_of = |block: &VisibleBlock| {
            let data =
                TerminalData::for_command(block.command.clone(), &block.output, block_status(block));
            status_marks(&paint(
                terminal_block(&data, TerminalFilter::default(), None, None),
                &app,
            ))
        };
        for (block, what) in [
            (&running, "a still-running block"),
            (&background, "a promoted-to-background block"),
        ] {
            assert_eq!(
                marks_of(block).0,
                Some(app.theme.color(ColorToken::Accent)),
                "{what} stands an accent pole, not a success"
            );
            assert!(!marks_of(block).1, "{what} is not washed as a failure");
        }
        assert_eq!(
            marks_of(&done).0,
            None,
            "a finished zero-exit block draws no pole"
        );
        assert!(!marks_of(&done).1, "and no failure wash");
        assert_eq!(
            marks_of(&failed).0,
            Some(app.theme.color(ColorToken::Error)),
            "a finished non-zero-exit block stands the error pole"
        );
        assert!(
            marks_of(&failed).1,
            "and washes the block in the error colour"
        );
    }

    /// R8: the pane's executed-command block reads the block's own status
    /// instead of asserting success. A command that is still running draws as
    /// running, and a finished non-zero exit draws as an error.
    #[test]
    fn the_executed_command_block_reads_the_blocks_status() {
        use goble_terminal::hooks::{encode_hook, CommandFinishedValue, PreexecValue};
        use goble_terminal::HookEvent;

        let session = |command: &str, exit: Option<i32>| {
            let mut emulator = Emulator::new(80, 24);
            emulator.feed(&encode_hook(&HookEvent::Bootstrapped(Default::default())));
            emulator.feed(&encode_hook(&HookEvent::Preexec(PreexecValue {
                command: Some(command.to_string()),
            })));
            emulator.feed(format!("{command}\r\noutput\r\n").as_bytes());
            if let Some(exit_code) = exit {
                emulator.feed(&encode_hook(&HookEvent::CommandFinished(
                    CommandFinishedValue {
                        exit_code,
                        ..Default::default()
                    },
                )));
            }
            TerminalSession::with_emulator(emulator)
        };

        let running = executed_command_block(&session("sleep 30", None))
            .expect("a running command still draws its block");
        assert_eq!(running.status, Some(TerminalStatus::Running));

        let failed = executed_command_block(&session("boom", Some(2)))
            .expect("a failed command draws its block");
        assert_eq!(failed.status, Some(TerminalStatus::Error));
    }

    /// A `RootView` mounted on a single shell (PTY) space, so the pane's own
    /// events, the app's real actions and the per-frame rebuild are what the
    /// test drives. The pane's session is detached (no pty): the pane has a
    /// real grid/session to draw and route against without spawning a shell.
    fn shell_root() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        use crate::root_view::RootView;
        use crate::ui::{Pane, PaneKind, Space};
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.spaces = vec![Space::new(
                "Shell",
                Pane::Leaf {
                    id: 1,
                    kind: PaneKind::Terminal,
                },
            )];
            s.active_space = 0;
            s.active_pane_id = 1;
            s.ensure_pane_controls();
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));
        }
        (Box::new(root) as Box<dyn Element>, state, dir)
    }

    /// A `RootView` on the default space (the chat pane), the surface the shell
    /// pane must match.
    fn chat_root() -> (Box<dyn Element>, tempfile::TempDir) {
        use crate::root_view::RootView;
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));
        }
        (Box::new(root) as Box<dyn Element>, dir)
    }

    /// The drawn runs of the pane's own area: a run left of the sidebar is the
    /// sidebar quoting a conversation, not the pane.
    fn pane_runs(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<(String, Vector2F)> {
        render_element(root, vec2f(1024.0, 768.0), app)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { origin, text, .. }
                    if origin.x >= crate::ui::SIDEBAR_WIDTH =>
                {
                    Some((text.clone(), *origin))
                }
                _ => None,
            })
            .collect()
    }

    fn pane_has(runs: &[(String, Vector2F)], text: &str) -> bool {
        runs.iter().any(|(t, _)| t == text)
    }

    /// The pane-wide rules of the pane's own area, top down: its separators.
    fn pane_rules(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<f32> {
        render_element(root, vec2f(1024.0, 768.0), app)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { rect, .. }
                    if rect.height() <= 2.0
                        && rect.width() > 200.0
                        && rect.min_x() >= crate::ui::SIDEBAR_WIDTH =>
                {
                    Some(rect.min_y())
                }
                _ => None,
            })
            .collect()
    }

    /// Dispatch one plain key through the whole app tree, so the pane's own
    /// routing is exercised where the app runs it.
    fn press(root: &mut Box<dyn Element>, app: &AppContext, name: &str) -> bool {
        let mut ctx = EventContext::default();
        root.dispatch_event(&key(name, ModifiersState::none()), &mut ctx, app)
    }

    /// R5: the shell pane's bottom bar is the chat surface's own composer, not a
    /// bespoke second one. It draws the shared `ChatComposer`'s placeholder —
    /// the very string the chat pane's composer draws — and the pty-only `❯`
    /// input line this replaces is gone.
    #[test]
    fn the_shells_bottom_bar_is_the_shared_composer() {
        let app = AppContext::default();
        let (mut root, _state, _dir) = shell_root();
        let (mut chat, _chat_dir) = chat_root();
        let placeholder = ChatComposer::new().placeholder().to_string();

        let shell_runs = pane_runs(&mut root, &app);
        assert!(
            pane_has(&shell_runs, &placeholder),
            "the shell pane draws the shared composer's placeholder {placeholder:?}: {shell_runs:?}"
        );
        assert!(
            !pane_has(&shell_runs, "❯"),
            "the bespoke terminal input line is gone: {shell_runs:?}"
        );

        let chat_runs = pane_runs(&mut chat, &app);
        assert!(
            pane_has(&chat_runs, &placeholder),
            "the chat pane draws that same composer: {chat_runs:?}"
        );

        // The bar carries the one instruction that applies to a command line:
        // Cmd/Ctrl+Enter leaves the shell for a new agent conversation, and that
        // chord is named under the bar. The agent rich input's own vocabulary —
        // what Enter runs, what `!` runs — does not describe this line, which is
        // a command already.
        assert!(
            pane_has(&shell_runs, "new conversation"),
            "the shell bar names the gesture that opens a new conversation: {shell_runs:?}"
        );
        for label in ["run", "agent", "send", "shell", "commands", "tasks"] {
            assert!(
                !pane_has(&shell_runs, label),
                "the shell bar draws no instruction {label:?}: {shell_runs:?}"
            );
        }
        assert!(
            pane_has(&chat_runs, "send") && pane_has(&chat_runs, "send to cloud"),
            "the chat pane draws its own instructions: {chat_runs:?}"
        );

        // The shell's one instruction is the exception to the pane's rule that
        // its instructions are the strip over the input's separator: it is
        // drawn inside the bar, under that separator, where the line it opens
        // from is typed.
        let hint = pane_runs(&mut root, &app)
            .into_iter()
            .find(|(text, _)| text == "new conversation")
            .map(|(_, origin)| origin.y)
            .expect("the shell bar names the gesture that opens a conversation");
        let separator = pane_rules(&mut root, &app)
            .into_iter()
            .filter(|y| *y < hint)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            separator.is_finite(),
            "the bar has a separator over it (hint at {hint})"
        );
        assert!(
                separator < hint,
                "the shell's instruction is inside the input, under its separator ({separator} against {hint})"
            );
    }

    /// The directory pill's menu is a window onto the machine: while it is open
    /// it lists the directories under this pane's working directory — sorted,
    /// hidden entries and plain files left out, the way up last — and while it is
    /// closed it reads nothing at all, so a dropped menu costs no disk.
    #[test]
    fn the_directory_menu_lists_the_panes_subdirectories_and_the_way_up() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        let home = tempfile::tempdir().expect("temp home");
        std::fs::create_dir(home.path().join("goble")).unwrap();
        std::fs::create_dir(home.path().join(".hidden")).unwrap();
        std::fs::write(home.path().join("notes.txt"), "not a directory").unwrap();
        let cwd = home.path().to_string_lossy().to_string();
        {
            let mut s = state.borrow_mut();
            s.set_pane_path(1, cwd);
        }

        let closed = pane_runs(&mut root, &app);
        assert!(
            !pane_has(&closed, "goble"),
            "a closed menu lists nothing: {closed:?}"
        );

        {
            let mut s = state.borrow_mut();
            let flag = s.pane_controls_mut(1).dir_menu_open.clone();
            *flag.borrow_mut() = true;
        }
        let open = pane_runs(&mut root, &app);
        assert!(
            pane_has(&open, "goble"),
            "the subdirectory is a row: {open:?}"
        );
        assert!(
            pane_has(&open, crate::ui::pickers::PARENT_LABEL),
            "the way up is a row: {open:?}"
        );
        assert!(
            !pane_has(&open, ".hidden") && !pane_has(&open, "notes.txt"),
            "only visible directories are rows: {open:?}"
        );
    }

    /// The branch pill's menu is the repository's own branch list, read while the
    /// menu is open: every local branch is a row, not just the one the pane is
    /// on. The other branch's name is the discriminator — the pill draws the
    /// current one whether the menu is open or not.
    #[test]
    fn the_branch_menu_lists_the_repositorys_local_branches() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        let Some(repo) = repository_on_two_branches() else {
            return; // no git on this machine: there is no list to read
        };
        let cwd = repo.path().to_string_lossy().to_string();
        {
            let mut s = state.borrow_mut();
            s.set_pane_path(1, cwd);
        }

        let closed = pane_runs(&mut root, &app);
        assert!(
            pane_has(&closed, "feature/x"),
            "the pill names the branch the repository is on: {closed:?}"
        );
        assert!(
            !pane_has(&closed, "main"),
            "a closed menu lists nothing: {closed:?}"
        );

        {
            let mut s = state.borrow_mut();
            let flag = s.pane_controls_mut(1).branch_menu_open.clone();
            *flag.borrow_mut() = true;
        }
        let open = pane_runs(&mut root, &app);
        assert!(
            pane_has(&open, "main") && pane_has(&open, "feature/x"),
            "both local branches are rows: {open:?}"
        );
    }

    /// A scratch repository with a commit on `main` and `feature/x` checked out.
    /// `None` when git is not on this machine, so the case has nothing to read
    /// rather than a failure to report.
    fn repository_on_two_branches() -> Option<tempfile::TempDir> {
        let dir = tempfile::tempdir().ok()?;
        let git = |args: &[&str]| -> bool {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        };
        if !git(&["init", "--initial-branch=main"]) {
            return None;
        }
        if !git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ]) {
            return None;
        }
        if !git(&["checkout", "-b", "feature/x"]) {
            return None;
        }
        Some(dir)
    }

    /// The wheel over the shell's grid follows the platform's sign, the same
    /// way the transcript and the sidebar do: winit reports how far the
    /// *content* should move and positive is down (AppKit's `scrollingDeltaY`
    /// and X11's button 4 both mean "content down"), so a positive delta walks
    /// back into the scrollback — the wheel-up a pager calls "scroll up" — and
    /// a negative one returns towards the live screen.
    #[test]
    fn the_wheels_sign_scrolls_the_panes_scrollback() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        {
            let s = state.borrow_mut();
            // More lines than the grid holds, so there is history to walk into.
            let mut emulator = Emulator::new(80, 24);
            for line in 0..120 {
                emulator.feed(format!("line {line}\r\n").as_bytes());
            }
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(emulator));
        }
        // One frame, so the pane has bounds and the wheel has somewhere to land.
        let _ = render_element(&mut root, vec2f(1024.0, 768.0), &app);

        let offset = || {
            state
                .borrow()
                .terminal
                .borrow()
                .sessions
                .get(&1)
                .map(|session| session.view().display_offset)
                .expect("the pane's session")
        };
        let wheel = |root: &mut Box<dyn Element>, dy: f32| {
            let mut ctx = EventContext::default();
            root.dispatch_event(
                &DispatchedEvent::MouseMove {
                    position: vec2f(600.0, 300.0),
                },
                &mut ctx,
                &app,
            );
            root.dispatch_event(
                &DispatchedEvent::Scroll {
                    delta: vec2f(0.0, dy),
                },
                &mut ctx,
                &app,
            );
        };

        assert_eq!(offset(), 0, "the pane opens at the live screen");
        wheel(&mut root, 60.0);
        let back = offset();
        assert!(
            back > 0,
            "a positive delta walks back into the scrollback: {back}"
        );
        wheel(&mut root, -60.0);
        assert!(
            offset() < back,
            "and a negative one returns towards the live screen: {} then {}",
            back,
            offset()
        );
    }

    /// The pane's bar carries one control beside the close X: the expand
    /// control that grows the pane over the panes space. Nothing opens a tray
    /// over the shell's output any more, so the shell keeps the pane's surface
    /// and the bar keeps one control.
    #[test]
    fn the_panes_bar_draws_the_expand_control_and_no_tray() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        {
            let s = state.borrow_mut();
            let mut emulator = Emulator::new(80, 24);
            emulator.feed(b"echo the-pane-keeps-its-output\r\nthe-pane-keeps-its-output\r\n");
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(emulator));
        }

        let window = vec2f(1024.0, 768.0);
        let commands = render_element(&mut root, window, &app);

        // The shell's output is on screen as the grid's per-glyph runs.
        let painted: String = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } if text.chars().count() == 1 => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            painted.contains("the-pane-keeps-its-output"),
            "the pane paints the shell's output: {painted:?}"
        );

        let icons: Vec<&String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name),
                _ => None,
            })
            .collect();
        assert!(
            icons.iter().any(|name| *name == "maximize-01"),
            "the pane's bar draws the expand control: {icons:?}"
        );
        assert!(
            !icons.iter().any(|name| *name == "dots-horizontal"),
            "and no tray trigger: {icons:?}"
        );

        let drawn: Vec<&String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        for gone in ["Copy", "Restart", "Rename", "Clear transcript", "Fullscreen"] {
            assert!(
                !drawn.iter().any(|text| *text == gone),
                "the tray label {gone} is not drawn over the output: {drawn:?}"
            );
        }
    }

    /// R5: the bar is the pane's only typing surface. An active shell pane hands
    /// it the keyboard in its own build, a click on the editor keeps it, and a
    /// click on the output above leaves it where it is: the terminal is where
    /// commands print, not where they are typed.
    #[test]
    fn an_active_shell_pane_keeps_the_keys_in_its_bar() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();

        // Lay the tree out first: a click is dispatched against the bounds the
        // bar was laid out with, and the placeholder's own origin is a point
        // inside the editor that drew it.
        let runs = pane_runs(&mut root, &app);
        let placeholder = ChatComposer::new().placeholder().to_string();
        let editor = runs
            .iter()
            .find(|(text, _)| text == &placeholder)
            .map(|(_, origin)| *origin)
            .expect("the bar drew its placeholder");

        // The pane's own build decided who holds the keys, and it chose the bar:
        // the keystroke edits the draft and the shell never sees it.
        assert!(press(&mut root, &app, "l"), "the bar takes the keystroke");
        assert_eq!(
            state.borrow().pane_sessions.get(&1).unwrap().draft,
            "l",
            "typing edits this pane's own draft"
        );
        assert_eq!(
            state.borrow().terminal.borrow().input(1),
            "",
            "the terminal never receives typed characters"
        );

        // A click on the bar's editor keeps the keys there.
        let mut ctx = EventContext::default();
        let click = DispatchedEvent::MouseDown {
            position: Vector2F::new(editor.x + 4.0, editor.y + 4.0),
            button: 0,
        };
        assert!(
            root.dispatch_event(&click, &mut ctx, &app),
            "the click lands on the bar"
        );
        assert!(
            press(&mut root, &app, "s"),
            "the editor still holds the keys"
        );

        // And a click on the shell's own grid is not a way to type either: the
        // editor keeps the keys, so the click cannot become terminal input.
        let grid = Vector2F::new(editor.x, editor.y - 200.0);
        let click = DispatchedEvent::MouseDown {
            position: grid,
            button: 0,
        };
        let _ = root.dispatch_event(&click, &mut ctx, &app);
        assert!(
            press(&mut root, &app, "a"),
            "a click on the output does not take the keys from the bar"
        );
        assert_eq!(
            state.borrow().pane_sessions.get(&1).unwrap().draft,
            "lsa",
            "every keystroke landed in the bar"
        );
        assert_eq!(
            state.borrow().terminal.borrow().input(1),
            "",
            "the click never reaches the shell as input"
        );
    }

    /// No runnable model: Cmd+Enter at the shell's bar still opens the agent
    /// view. The view needs a transcript to draw, so a conversation is bound —
    /// but nothing is sent into it, and the notice band is what names the
    /// missing model. The bar keeps the draft, since no turn can take it.
    #[test]
    fn a_prompt_with_no_model_still_opens_the_agent_view() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        let _ = pane_runs(&mut root, &app);

        let typed = "explain the last command";
        for ch in typed.chars() {
            assert!(press(&mut root, &app, &ch.to_string()));
        }
        let cmd_enter = key(
            "Enter",
            ModifiersState {
                command: true,
                ..ModifiersState::default()
            },
        );
        let mut ctx = EventContext::default();
        assert!(root.dispatch_event(&cmd_enter, &mut ctx, &app));

        let s = state.borrow();
        assert!(
            s.pane_controls(1).harness_mode,
            "the switch to terminal + agent works with no model configured"
        );
        let conversation_id = s
            .pane_conversation_id(1)
            .expect("the agent view has a conversation of its own to draw");
        assert_eq!(
            s.pane_view(1),
            BlockView::Agent { conversation_id },
            "the pane shows the agent view"
        );
        assert!(
            s.show_llm_key_banner,
            "the notice band names the missing model"
        );
        assert_eq!(s.llm_notice_heading(), "No API key configured");
        assert_eq!(
            s.pane_sessions.get(&1).unwrap().draft,
            typed,
            "the bar keeps what was typed"
        );
    }

    /// A `cd` typed at the pane's own prompt prints nothing, so the move reaches
    /// the rich input on the shell-integration channel alone: the prompt that
    /// followed reports its directory, the frame's pump reads it into the pane's
    /// session, and the directory pill — which draws that session's path —
    /// follows on the frame after. Nothing reads the typed line or the output.
    #[test]
    fn a_reported_cwd_reaches_the_panes_directory_pill() {
        use goble_terminal::hooks::{encode_hook, PrecmdValue};
        use goble_terminal::HookEvent;

        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        {
            // The hook the shell sends at the prompt that followed its `cd`,
            // already in the session the pane draws from.
            let mut emulator = Emulator::new(80, 24);
            emulator.feed(&encode_hook(&HookEvent::Precmd(PrecmdValue {
                pwd: Some("/b".to_string()),
                ..Default::default()
            })));
            let s = state.borrow_mut();
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(emulator));
        }

        // The first frame's pump is where the session reads the report; the frame
        // after it is where the pane's snapshot is built from what it read.
        let _ = pane_runs(&mut root, &app);
        let runs = pane_runs(&mut root, &app);

        assert_eq!(
            state.borrow().composer_path,
            "/b",
            "the pane's directory followed its own shell"
        );
        assert!(
            pane_has(&runs, "/b"),
            "the rich input's directory pill draws it: {runs:?}"
        );
    }

    /// The shell's bar is the pane's submit line for *commands*: Enter runs the
    /// draft in this pane's own PTY and leaves the pane on the shell — it does
    /// not open the harness, because the agent view has a composer of its own
    /// that takes prompts. Cmd+Enter is what opens that view. The bar is the
    /// pane's only typing surface, so it holds the keys whenever the pane is
    /// active and no full-screen program owns the screen; Esc puts the shell
    /// back with the conversation's card left behind.
    #[test]
    fn the_shells_bar_runs_its_draft_as_a_command_and_cmd_enter_opens_the_agent_view() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();

        // A model is configured here, because Cmd+Enter below opens a *new*
        // conversation and nothing is created while nothing could answer it
        // (that refusal is covered in `terminal/tests::a_prompt_with_no_model`).
        let rt = tokio::runtime::Runtime::new().expect("build runtime");
        let _guard = rt.enter();
        {
            let mut s = state.borrow_mut();
            s.settings_llm_provider = "mock".to_string();
            s.settings_llm_api_key = "test-key".to_string();
        }

        // No focus flag is set by hand: an active shell pane's own build gives
        // the bar the keyboard, because typing happens only there.
        let _ = pane_runs(&mut root, &app);
        let typed = "ls -la";
        for ch in typed.chars() {
            assert!(
                press(&mut root, &app, &ch.to_string()),
                "the focused bar takes {ch:?}"
            );
        }
        assert_eq!(
            state.borrow().pane_sessions.get(&1).unwrap().draft,
            typed,
            "typing edits this pane's own draft"
        );

        // Enter is the command affordance: the draft is submitted to this
        // pane's PTY and the pane stays on the shell. An agent turn here would
        // have opened the harness and bound this pane a conversation.
        assert!(press(&mut root, &app, "Enter"));
        assert!(
            !state.borrow().pane_controls(1).harness_mode,
            "Enter runs a command instead of opening the harness"
        );
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Terminal,
            "the pane is still the shell after a command"
        );
        assert!(
            state.borrow().pane_conversation_id(1).is_none(),
            "a command does not bind this pane an agent conversation"
        );
        assert_eq!(
            state.borrow().pane_sessions.get(&1).unwrap().draft,
            "",
            "the bar clears after the command"
        );

        // Cmd+Enter is the way to the agent view: a draft typed at the same bar
        // opens the harness on this pane's conversation and goes to the agent.
        for ch in "explain the last command".chars() {
            assert!(press(&mut root, &app, &ch.to_string()));
        }
        let cmd_enter = key(
            "Enter",
            ModifiersState {
                command: true,
                ..ModifiersState::default()
            },
        );
        let mut ctx = EventContext::default();
        assert!(root.dispatch_event(&cmd_enter, &mut ctx, &app));
        assert!(
            state.borrow().pane_controls(1).harness_mode,
            "Cmd+Enter opens the harness"
        );
        let conversation_id = state
            .borrow()
            .pane_conversation_id(1)
            .expect("the pane bound a conversation of its own");
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent {
                conversation_id: conversation_id.clone()
            },
            "the harness draws the conversation's agent view"
        );
        // The open harness is the shared agent view, and the shared composer is
        // still pinned at the bottom of it.
        let runs = pane_runs(&mut root, &app);
        assert!(
            pane_has(&runs, ChatComposer::new().placeholder()),
            "the agent view carries the shared composer: {runs:?}"
        );

        // Esc returns the pane to the shell; the card stays in its block list.
        assert!(press(&mut root, &app, "Escape"));
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Terminal,
            "Esc puts the shell back"
        );
        let cards: Vec<String> = state
            .borrow()
            .pane_terminal_view(1)
            .into_iter()
            .filter_map(|block| block.card.map(|card| card.conversation_id))
            .collect();
        assert_eq!(
            cards,
            vec![conversation_id.clone()],
            "the conversation's card stays in the terminal scrollback"
        );

        // The bar takes the keys back: it is the pane's only typing surface, so
        // the keystrokes that once reached the shell now edit the draft, and the
        // shell's input mirror stays empty.
        let runs = pane_runs(&mut root, &app);
        assert!(
            pane_has(&runs, ChatComposer::new().placeholder()),
            "the shell's bar is back at the bottom: {runs:?}"
        );
        assert!(press(&mut root, &app, "l"));
        assert!(press(&mut root, &app, "s"));
        assert_eq!(
            state.borrow().pane_sessions.get(&1).unwrap().draft,
            "ls",
            "the bar takes the keys"
        );
        assert_eq!(
            state.borrow().terminal.borrow().input(1),
            "",
            "the terminal never receives typed characters"
        );

        // Clicking the card reopens that conversation's agent view. The card's
        // accent rail is where the card landed.
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let rail = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect { rect, .. }
                    if rect.width() == CARD_RAIL_WIDTH && rect.min_x() >= crate::ui::SIDEBAR_WIDTH =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the terminal view draws the conversation card");
        let position = Vector2F::new(rail.min_x() + 20.0, rail.min_y() + 8.0);
        let mut ctx = EventContext::default();
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position,
                button: 0
            },
            &mut ctx,
            &app
        ));
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent { conversation_id },
            "clicking the card reopens the conversation's agent view"
        );
    }

    /// R6: the app's conversation card is A8's `ConversationCard` element, not
    /// a private bordered container. It paints its status rail and no border.
    #[test]
    fn the_conversation_card_is_card_free() {
        let app = AppContext::default();
        let mut card = build_card(&AgentViewCard {
            conversation_id: "c1".to_string(),
            label: "Fix the build".to_string(),
        });
        let commands = render_element(&mut card, vec2f(420.0, 40.0), &app);

        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(strokes, 0, "the conversation card draws no border");

        // The accent rail is the card element's own mark, not a bordered box.
        let rail = commands.iter().find_map(|c| match c {
            RenderCommand::FillRect {
                rect,
                corner_radius,
                ..
            } if rect.width() == CARD_RAIL_WIDTH => Some(*corner_radius),
            _ => None,
        });
        assert_eq!(
            rail,
            Some(0.0),
            "the card element paints its square status rail"
        );
    }

    /// R3/B3: a terminal command is a section of its own, headed by warp-new's
    /// block label — one mono prompt row carrying the directory it ran in, the
    /// branch and how long it took — with the command itself drawn once, on the
    /// section's `❯` line. (This replaces the assertion that each part was its
    /// own label: the header is one prompt row now, so it asserts that row.)
    #[test]
    fn a_command_is_drawn_as_a_section_with_its_directory_and_duration() {
        use goble_terminal::hooks::{encode_hook, CommandFinishedValue, PrecmdValue, PreexecValue};
        use goble_terminal::HookEvent;

        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        let home = std::env::var("HOME").expect("a home directory for the ~ path");
        let workdir = format!("{home}/Projects/goble");
        {
            let mut emulator = Emulator::new(80, 24);
            emulator.feed(&encode_hook(&HookEvent::Bootstrapped(Default::default())));
            emulator.feed(&encode_hook(&HookEvent::Precmd(PrecmdValue {
                pwd: Some(workdir.clone()),
                git_branch: Some("main".to_string()),
                ..Default::default()
            })));
            emulator.feed(&encode_hook(&HookEvent::Preexec(PreexecValue {
                command: Some("cargo test".to_string()),
            })));
            emulator.feed(b"42 passed\r\n");
            emulator.feed(&encode_hook(&HookEvent::CommandFinished(
                CommandFinishedValue {
                    exit_code: 0,
                    ..Default::default()
                },
            )));
            state
                .borrow_mut()
                .terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(emulator));
        }

        let runs = pane_runs(&mut root, &app);
        let prompt = runs
            .iter()
            .map(|(text, _)| text.clone())
            .find(|text| text.starts_with("~/Projects/goble"))
            .unwrap_or_else(|| panic!("the section header is one prompt row: {runs:?}"));
        assert!(
            prompt.contains("git:(main)"),
            "the prompt row names the branch it ran on: {prompt:?}"
        );
        assert!(
            prompt.ends_with("s)") && prompt.contains(" ("),
            "the prompt row carries the command's duration in parentheses: {prompt:?}"
        );
        for separate in ["~/Projects/goble", "git:(main)"] {
            assert!(
                !pane_has(&runs, separate),
                "the prompt row is one row, not a label per part, but {separate} was drawn alone: {runs:?}"
            );
        }
        assert!(
            runs.iter().any(|(text, _)| text.contains("cargo")),
            "the section draws the command: {runs:?}"
        );
        assert!(
            runs.iter().any(|(text, _)| text.contains("42 passed")),
            "the section draws the command's output: {runs:?}"
        );
        // The shell's own prompt is hidden: the block view draws sections, not
        // the raw screen, so no bare `$` prompt line is left in the pane.
        assert!(
            !pane_has(&runs, "$"),
            "the block view hides the shell's prompt: {runs:?}"
        );
    }

    /// The duration reads the way warp's block label writes it: seconds under a
    /// minute, minutes and seconds under an hour, hours above.
    #[test]
    fn a_duration_reads_in_seconds_minutes_and_hours() {
        use super::blocks::duration_text;
        use std::time::Duration;

        assert_eq!(duration_text(Duration::from_millis(1240)), "(1.240s)");
        assert_eq!(duration_text(Duration::from_millis(68_920)), "(1m 8.92s)");
        assert_eq!(
            duration_text(Duration::from_secs(3 * 3600 + 4 * 60 + 12)),
            "(3h 4m 12s)"
        );
    }

    /// Every drawn text, in draw order.
    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The block's own status marks as `(pole colour, washed)`: the pole a block
    /// stands on its left edge and whether the whole block is tinted under its
    /// content. A block that is neither running nor failed draws neither.
    fn status_marks(commands: &[RenderCommand]) -> (Option<ColorU>, bool) {
        let mut pole = None;
        let mut wash = false;
        for command in commands {
            if let RenderCommand::FillRect { rect, color, .. } = command {
                if (rect.width() - 3.0).abs() < 0.01 {
                    pole = Some(*color);
                } else if rect.width() > 500.0 && rect.height() > 20.0 {
                    wash = true;
                }
            }
        }
        (pole, wash)
    }

    /// The session a pane draws a section from: one command that ran and printed
    /// `output`, delimited by the shell-integration hooks a real shell sends, so
    /// the pane's block list has a section with a lifecycle of its own.
    fn session_that_ran(command: &str, output: &str) -> TerminalSession {
        use goble_terminal::hooks::{encode_hook, CommandFinishedValue, PreexecValue};
        use goble_terminal::HookEvent;

        let mut emulator = Emulator::new(80, 24);
        emulator.feed(&encode_hook(&HookEvent::Bootstrapped(Default::default())));
        emulator.feed(&encode_hook(&HookEvent::Preexec(PreexecValue {
            command: Some(command.to_string()),
        })));
        emulator.feed(format!("{command}\r\n{output}\r\n").as_bytes());
        emulator.feed(&encode_hook(&HookEvent::CommandFinished(
            CommandFinishedValue {
                exit_code: 0,
                ..Default::default()
            },
        )));
        TerminalSession::with_emulator(emulator)
    }

    /// A pane's own view over one session, drawing through `plumbing` — the
    /// app's own per-block filter map and this pane's whole-output filter. The
    /// registry comes back with it, so a test can read what reached the shell.
    fn view_over(
        session: TerminalSession,
        pane_id: u64,
        plumbing: TerminalBlockPlumbing,
    ) -> (TerminalView, Rc<RefCell<TerminalRegistry>>) {
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        terminal.borrow_mut().sessions.insert(pane_id, session);
        let view = TerminalView::new(
            Rc::clone(&terminal),
            pane_id,
            "/tmp".to_string(),
            true,
            BlockView::Terminal,
            // The shell's own state: the rich input holds the keyboard.
            true,
            Text::new("bar").finish(),
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
            Rc::new(RefCell::new(|_: u64, _: bool| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
        )
        .with_block_plumbing(plumbing);
        (view, terminal)
    }

    /// The `n of m` a filter bar draws, as the pair it names.
    fn count_pair(text: &str) -> (usize, usize) {
        let (matched, total) = text.split_once(" of ").expect("a count reads '<n> of <n>'");
        (
            matched.parse().expect("matches"),
            total.parse().expect("total"),
        )
    }

    fn block_filters() -> Rc<RefCell<HashMap<String, TerminalFilter>>> {
        Rc::new(RefCell::new(HashMap::new()))
    }

    /// C: Cmd+F opens the pane's whole-output filter — one bar over every block
    /// the pane draws, with a real field the keys go into — and what is typed
    /// narrows the output of them all.
    #[test]
    fn cmd_f_opens_the_panes_whole_output_filter_and_its_query_narrows_the_output() {
        let app = AppContext::default();
        let global = TerminalFilter::default();
        let (pane, terminal) = view_over(
            session_that_ran("printf hi", "alpha line\r\nbeta line"),
            7,
            TerminalBlockPlumbing::new(block_filters(), Some(global.clone()), None),
        );
        let mut pane: Box<dyn Element> = Box::new(pane);
        let mut ctx = EventContext::default();
        let window = vec2f(900.0, 500.0);

        let closed = texts(&render_element(&mut pane, window, &app));
        assert!(
            !closed.iter().any(|t| t == "Filter terminal output"),
            "a closed filter draws no bar: {closed:?}"
        );
        assert!(
            closed.iter().any(|t| t == "alpha line") && closed.iter().any(|t| t == "beta line"),
            "the pane draws its block's whole output: {closed:?}"
        );

        // Cmd+F: the pane takes the chord before the tree, so the composer it
        // hands the keys to cannot swallow it.
        let cmd_f = key(
            "f",
            ModifiersState {
                command: true,
                ..ModifiersState::default()
            },
        );
        assert!(
            pane.dispatch_event(&cmd_f, &mut ctx, &app),
            "the pane takes Cmd+F"
        );
        assert!(global.is_open(), "Cmd+F shows the whole-output filter");
        assert!(
            *global.focused.borrow(),
            "and puts the caret in its field, which is what types into it"
        );

        let open = texts(&render_element(&mut pane, window, &app));
        assert!(
            open.iter().any(|t| t == "Filter terminal output"),
            "the bar names what it filters: {open:?}"
        );
        let count = |drawn: &[String]| {
            drawn
                .iter()
                .find(|t| t.contains(" of "))
                .cloned()
                .expect("the bar counts the lines it filters")
        };
        let before = count(&open);
        let (matched_before, total) = count_pair(&before);
        assert_eq!(
            matched_before, total,
            "an empty query keeps every line the pane's blocks carry"
        );
        assert!(total > 1, "the pane's block carries more than one line");

        // Typing goes into the bar's field — the pane routes the keys into the
        // tree while that field holds the caret — never to the shell.
        for ch in ["b", "e", "t", "a"] {
            assert!(
                pane.dispatch_event(&key(ch, ModifiersState::none()), &mut ctx, &app),
                "the bar's field takes the keystroke {ch:?}"
            );
        }
        assert_eq!(global.query.borrow().as_str(), "beta");
        assert_eq!(
            terminal.borrow().input(7),
            "",
            "the shell never receives a keystroke meant for the filter"
        );

        let narrowed = texts(&render_element(&mut pane, window, &app));
        assert!(
            narrowed.iter().any(|t| t == "beta line"),
            "the matching line is drawn: {narrowed:?}"
        );
        assert!(
            !narrowed.iter().any(|t| t == "alpha line"),
            "every block's non-matching lines are hidden: {narrowed:?}"
        );
        let (matched_after, total_after) = count_pair(&count(&narrowed));
        assert_eq!(
            total_after, total,
            "the count still measures the whole output"
        );
        assert_eq!(matched_after, 1, "and counts what the query left");
    }

    /// C: each pane reads its own whole-output filter, so a query typed in one
    /// pane leaves its sibling's output alone.
    #[test]
    fn a_general_filter_in_one_pane_leaves_another_panes_output_alone() {
        let app = AppContext::default();
        let filters = block_filters();
        let mine = TerminalFilter::default();
        mine.set_query("beta");
        let (mine_pane, _mine_terminal) = view_over(
            session_that_ran("printf hi", "alpha line\r\nbeta line"),
            7,
            TerminalBlockPlumbing::new(filters.clone(), Some(mine), None),
        );
        let (theirs_pane, _theirs_terminal) = view_over(
            session_that_ran("printf hi", "alpha line\r\nbeta line"),
            8,
            TerminalBlockPlumbing::new(filters, Some(TerminalFilter::default()), None),
        );
        let mut mine_pane: Box<dyn Element> = Box::new(mine_pane);
        let mut theirs_pane: Box<dyn Element> = Box::new(theirs_pane);

        let window = vec2f(900.0, 500.0);
        let mine_drawn = texts(&render_element(&mut mine_pane, window, &app));
        assert!(
            mine_drawn.iter().any(|t| t == "beta line")
                && !mine_drawn.iter().any(|t| t == "alpha line"),
            "the queried pane keeps only its matching lines: {mine_drawn:?}"
        );
        let theirs_drawn = texts(&render_element(&mut theirs_pane, window, &app));
        assert!(
            theirs_drawn.iter().any(|t| t == "alpha line")
                && theirs_drawn.iter().any(|t| t == "beta line"),
            "the sibling pane draws every line: {theirs_drawn:?}"
        );
    }

    /// B: Cmd+Shift+F (warp's per-block chord) opens the filter of the block the
    /// pointer is over, and the bar's field takes what is typed into it.
    #[test]
    fn cmd_shift_f_opens_the_filter_of_the_block_under_the_pointer() {
        let app = AppContext::default();
        let plumbing = TerminalBlockPlumbing::new(block_filters(), None, None);
        let (pane, _terminal) = view_over(
            session_that_ran("printf hi", "alpha line\r\nbeta line"),
            7,
            plumbing.clone(),
        );
        let mut pane: Box<dyn Element> = Box::new(pane);
        let window = vec2f(900.0, 500.0);
        let mut ctx = EventContext::default();
        let chord = key(
            "f",
            ModifiersState {
                command: true,
                shift: true,
                ..ModifiersState::default()
            },
        );

        // With the pointer off every block there is nothing to filter.
        let commands = render_element(&mut pane, window, &app);
        pane.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(450.0, 4.0),
            },
            &mut ctx,
            &app,
        );
        assert!(plumbing.hovered_filter().is_none());
        pane.dispatch_event(&chord, &mut ctx, &app);
        assert!(
            plumbing.hovered_filter().is_none(),
            "a chord with no block under the pointer opens nothing"
        );

        // Over the block: the chord opens that block's own filter bar.
        let at = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::DrawText { origin, text, .. } if text == "beta line" => Some(*origin),
                _ => None,
            })
            .expect("the pane draws its block's output");
        pane.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(at.x + 2.0, at.y + 4.0),
            },
            &mut ctx,
            &app,
        );
        let block = plumbing
            .hovered_filter()
            .expect("the pointer is over the block");
        assert!(
            pane.dispatch_event(&chord, &mut ctx, &app),
            "the pane takes Cmd+Shift+F"
        );
        assert!(
            block.is_open(),
            "the chord shows the filter of the block under the pointer"
        );

        let drawn = texts(&render_element(&mut pane, window, &app));
        assert!(
            drawn.iter().any(|t| t == "Filter block output"),
            "the block's own bar is drawn with its placeholder: {drawn:?}"
        );
    }
