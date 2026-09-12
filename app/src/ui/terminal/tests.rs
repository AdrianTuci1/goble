use super::blocks::block_status;
use super::cards::build_card;
use crate::emulator::{AgentViewCard, VisibleBlock};
use crate::terminal::{TerminalRegistry, TerminalSession};
use goble_ui::elements::{
    AppContext, EventContext, TerminalData, TerminalGrid, TerminalStatus, Text,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::theme::FontFamily;

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
    /// behaviour it encoded is the same, on the surface that survives.)
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
        let color_of = |block: &VisibleBlock| {
            let data = TerminalData::for_command(
                block.command.clone(),
                &block.output,
                block_status(block),
            );
            text_runs(&paint(
                terminal_block(&data, TerminalFilter::default(), None, None),
                &app,
            ))
            .into_iter()
            .find(|(text, ..)| text == &block.command)
            .map(|(_, color, ..)| color)
        };
        assert_eq!(
            color_of(&running),
            Some(app.theme.color(ColorToken::Accent)),
            "a still-running block is drawn as running, not as a success"
        );
        assert_eq!(
            color_of(&background),
            Some(app.theme.color(ColorToken::Accent)),
            "a promoted-to-background block is still not a success"
        );
        assert_eq!(
            color_of(&done),
            Some(app.theme.color(ColorToken::Muted)),
            "a finished zero-exit block draws the muted resting title"
        );
        assert_eq!(
            color_of(&failed),
            Some(app.theme.color(ColorToken::Error)),
            "a finished non-zero-exit block is an error"
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
            s.settings_overlay_open = false;
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
            s.settings_overlay_open = false;
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
    }

    /// The pane's topbar tray is an overlay, not another child of the pane's
    /// column: with the dots open, the panel must be painted after the shell's
    /// output so it covers text instead of being covered by it, and it must open
    /// leftwards from the dots, which sit at the pane's right edge, so the whole
    /// panel stays inside the window.
    #[test]
    fn the_panes_tray_paints_over_the_output_and_stays_inside_the_window() {
        let app = AppContext::default();
        let (mut root, state, _dir) = shell_root();
        {
            let mut s = state.borrow_mut();
            // Output under the tray, so "paints over it" is a claim about text.
            let mut emulator = Emulator::new(80, 24);
            emulator.feed(b"echo painted-under-the-tray\r\npainted-under-the-tray\r\n");
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(emulator));
            // The dots' open flag is app-owned state; a click on the trigger
            // flips it, so opening it here is what that click leaves behind.
            s.agent_header_menus
                .insert(1, Rc::new(RefCell::new(true)));
        }

        let window = vec2f(1024.0, 768.0);
        let commands = render_element(&mut root, window, &app);

        // The panel's own surface: the raised background the popup paints. The
        // header band uses `Surface`, so this rect is the panel's.
        let raised = app.theme.color(goble_ui::theme::ColorToken::SurfaceRaised);
        let (panel_index, panel) = commands
            .iter()
            .enumerate()
            .find_map(|(index, command)| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == raised && rect.height() > 100.0 =>
                {
                    Some((index, *rect))
                }
                _ => None,
            })
            .expect("the open tray paints its panel");
        // The shell's output is on screen as the grid's per-glyph runs, so a
        // panel painted after the last of them covers text, not just background.
        let grid_glyphs: Vec<&String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } if text.chars().count() == 1 => Some(text),
                _ => None,
            })
            .collect();
        let painted: String = grid_glyphs.iter().map(|text| text.as_str()).collect();
        assert!(
            painted.contains("painted-under-the-tray"),
            "the pane paints the shell's output: {painted:?}"
        );
        let output_index = commands
            .iter()
            .rposition(|command| {
                matches!(command, RenderCommand::DrawText { text, .. } if text.chars().count() == 1)
            })
            .expect("the pane paints the shell's output");
        let bar_index = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    RenderCommand::DrawText { text, .. } if text == ChatComposer::new().placeholder()
                )
            })
            .expect("the pane paints its rich input");
        let tray_text_index = commands
            .iter()
            .position(|command| {
                matches!(command, RenderCommand::DrawText { text, .. } if text == "Clear transcript")
            })
            .expect("the open tray paints its items");
        assert!(
            panel_index > output_index && panel_index > bar_index,
            "the tray paints after the pane's own content: panel {panel_index}, output {output_index}, bar {bar_index}"
        );
        assert!(
            panel_index < tray_text_index,
            "the panel's items paint on its own surface: panel {panel_index}, items {tray_text_index}"
        );

        // Leftwards from the dots, which sit at the pane's right edge: the panel
        // stops short of the window's right edge and stays inside the pane.
        assert!(
            panel.min_x() >= crate::ui::SIDEBAR_WIDTH,
            "the tray opened leftwards into the pane: {panel:?}"
        );
        assert!(
            panel.max_x() <= window.x - 32.0 && panel.max_y() <= window.y,
            "the tray stops short of the window's right edge: {panel:?}"
        );
        assert!(panel.min_y() >= 0.0, "the tray is inside the window: {panel:?}");
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
        assert!(press(&mut root, &app, "s"), "the editor still holds the keys");

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
                    if rect.width() == CARD_RAIL_WIDTH
                        && rect.min_x() >= crate::ui::SIDEBAR_WIDTH =>
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

    /// R3/B3: a terminal command is a section of its own, with the directory it
    /// ran in and how long it took on the section's top line (warp-new's block
    /// label), and the command itself drawn once, on the section's `❯` line.
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
        assert!(
            pane_has(&runs, "~/Projects/goble"),
            "the section header names the directory it ran in: {runs:?}"
        );
        assert!(
            pane_has(&runs, "git:(main)"),
            "the section header names the branch: {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|(text, _)| text.starts_with('(') && text.ends_with("s)")),
            "the section header carries the command's duration: {runs:?}"
        );
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
        assert_eq!(duration_text(Duration::from_secs(3 * 3600 + 4 * 60 + 12)), "(3h 4m 12s)");
    }
