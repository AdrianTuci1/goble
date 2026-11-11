use super::*;

    use crate::emulator::Emulator;
    use crate::terminal::TerminalSession;
    use goble_core::store::Store;
    use goble_desktop_service::{SubAgentProgressEvent, SubAgentSpawnedEvent, ThreadStore};
    use goble_terminal::blocks::BlockView;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    /// The app mounted over a parent conversation whose pane has spawned one
    /// running child.
    struct Fixture {
        root: RootView,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let parent = desktop
            .create_chat("Parent conversation", None, None)
            .expect("parent chat");
        desktop
            .add_chat_message(&parent, "user", "PARENT TRANSCRIPT MARKER")
            .expect("parent row");
        let now = "2026-09-11T10:00:00Z".to_string();
        {
            let store = desktop.store_clone();
            store
                .insert_subagent_chat(
                    "conv-child-1",
                    "reviewer: audit the migration",
                    &parent,
                    None,
                    None,
                    &now,
                    &now,
                )
                .expect("child chat");
        }
        desktop
            .add_chat_message("conv-child-1", "assistant", "CHILD CONVERSATION MARKER")
            .expect("child row");

        let root = RootView::new(&AppContext::default(), &desktop, None);
        {
            let state = root.state_rc();
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.pane_sessions
                .get_mut(&1)
                .expect("the default pane session exists")
                .conversation_id = parent.clone();
            s.terminal
                .borrow_mut()
                .sessions
                .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));
            s.pane_controls_mut(1).model = "deepseek".to_string();
            // The parent's turn is in flight while it delegates, as it is when
            // a child is spawned: an ended row stays in the strip until that
            // turn ends.
            s.pane_runtime.entry(1).or_default().busy = true;
            s.apply_subagent_spawned(&SubAgentSpawnedEvent {
                chat_id: parent.clone(),
                subagent_id: "conv-child-1".into(),
                subagent_type: "reviewer".into(),
                description: "audit the migration".into(),
                parent_call_id: "call_spawn".into(),
                run_in_background: true,
            });
            s.apply_subagent_progress(&SubAgentProgressEvent {
                chat_id: parent.clone(),
                subagent_id: "conv-child-1".into(),
                status: "running".into(),
                activity: "reading the schema".into(),
                turns: 2,
                tool_calls: 4,
                tokens: 1234,
                duration_ms: 4500,
            });
            s.refresh_messages(&desktop);
        }
        Fixture {
            root,
            _dir: dir,
        }
    }

    /// Every run the pane draws, excluding the sidebar (which quotes the
    /// selected conversation's own rows).
    fn pane_commands(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        render_element(root, vec2f(1024.0, 768.0), app)
            .into_iter()
            .filter(|command| match command {
                RenderCommand::DrawText { origin, .. }
                | RenderCommand::DrawIcon { origin, .. } => {
                    origin.x >= crate::ui::SIDEBAR_WIDTH
                }
                _ => true,
            })
            .collect()
    }

    fn pane_texts(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<String> {
        pane_commands(root, app)
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The top-left corner of the pane run `label`.
    fn run_origin(commands: &[RenderCommand], label: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the pane draws the {label:?} run"))
    }

    /// The first of `labels` the header draws: a conversation's name, or the
    /// placeholder while the pane has no subject yet.
    fn header_origin(commands: &[RenderCommand], labels: &[&str]) -> Vector2F {
        labels
            .iter()
            .find_map(|label| {
                commands.iter().find_map(|command| match command {
                    RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
                    _ => None,
                })
            })
            .unwrap_or_else(|| panic!("the pane draws its header title: {labels:?}"))
    }

    /// The centre of a drawn icon, optionally the one nearest `near_y` — the
    /// strip and the transcript both draw icons, so its row is what names it.
    fn icon_centre(commands: &[RenderCommand], name: &str, near_y: f32) -> Vector2F {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, name: drawn, size, ..
                } if drawn == name => Some(*origin + vec2f(size / 2.0, size / 2.0)),
                _ => None,
            })
            .min_by(|a, b| {
                (a.y - near_y)
                    .abs()
                    .total_cmp(&(b.y - near_y).abs())
            })
            .unwrap_or_else(|| panic!("the pane draws the {name:?} icon"))
    }

    fn click(root: &mut Box<dyn Element>, app: &AppContext, position: Vector2F) {
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
            root.dispatch_event(&event, &mut ctx, app);
        }
    }

    /// The strip is chrome under the pane's own header: it names the group, its
    /// count, and each running child with the pane's model and its elapsed
    /// time. The transcript below it carries no controls of its own.
    #[test]
    fn the_panel_draws_under_the_agent_header_with_its_count_model_and_elapsed() {
        let Fixture { root, .. } = fixture();
        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        let commands = pane_commands(&mut root, &app);

        let header_y = header_origin(
            &commands,
            &["Parent conversation", crate::state::NEW_AGENT_TAB_LABEL],
        )
        .y;
        let strip_y = run_origin(&commands, "Subagents").y;
        let transcript_y = run_origin(&commands, "PARENT TRANSCRIPT MARKER").y;
        assert!(
            strip_y > header_y && strip_y < transcript_y,
            "the strip sits under the header ({header_y}) and above the \
             transcript ({transcript_y}), got {strip_y}"
        );

        let texts = pane_texts(&mut root, &app);
        assert!(
            texts.iter().any(|text| text == "1"),
            "the strip states how many children it lists, got {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "deepseek"),
            "the row names the pane's model, got {texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "4.5s"),
            "the row states the child's elapsed time, got {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|text| text.contains("audit the migration")),
            "the row names the child's description, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Stop" || text == "open child"),
            "the transcript row draws no controls, got {texts:?}"
        );
    }

    /// A click anywhere on the header row folds the strip: the children leave,
    /// the word and the count stay, and the chevron turns to the right.
    #[test]
    fn the_chevron_folds_the_panel() {
        let Fixture { root, .. } = fixture();
        let state = root.state_rc();
        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        let commands = pane_commands(&mut root, &app);
        let origin = run_origin(&commands, "Subagents");
        click(&mut root, &app, origin + vec2f(2.0, 4.0));

        assert!(
            !state.borrow().sub_agent_panel_open,
            "the header row closes the strip"
        );
        let commands = pane_commands(&mut root, &app);
        let texts: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|text| text == "Subagents"),
            "a folded strip keeps its header, got {texts:?}"
        );
        assert!(
            !texts
                .iter()
                .any(|text| text.contains("audit the migration")),
            "a folded strip draws none of its rows, got {texts:?}"
        );
        let strip_y = run_origin(&commands, "Subagents").y;
        let chevron = icon_centre(&commands, "chevron-right", strip_y);
        assert!(
            (chevron.y - strip_y).abs() < 12.0,
            "a folded strip turns its chevron to the right: {chevron:?} against {strip_y}"
        );
    }

    /// The stop control in the strip cancels the child it belongs to: the
    /// child's turn is cancelled and its row reads `cancelled` with the time it
    /// ran, with no stop left to press.
    #[test]
    fn the_panels_stop_cancels_the_child_and_its_row_reads_cancelled() {
        let Fixture { root, .. } = fixture();
        let state = root.state_rc();
        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        let commands = pane_commands(&mut root, &app);
        let strip_y = run_origin(&commands, "Subagents").y;
        let stop = icon_centre(&commands, "x-close", strip_y + 19.0);
        click(&mut root, &app, stop);

        let status = state
            .borrow()
            .pane_runtime
            .get(&1)
            .and_then(|rt| rt.sub_agents.get("conv-child-1"))
            .map(|record| record.row.status)
            .expect("the pane still holds the child's record");
        assert_eq!(
            status,
            goble_ui::SubAgentRowStatus::Cancelled,
            "the strip's stop cancels the child it names"
        );

        let commands = pane_commands(&mut root, &app);
        let texts: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|text| text.contains("cancelled in 4.5s")),
            "the row reads its cancellation, got {texts:?}"
        );
        // The strip's own band is below its header word, so a stop anywhere
        // under that word would be the cancelled child's; the pane bar's close
        // control sits above it.
        let strip_y = run_origin(&commands, "Subagents").y;
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { origin, name, .. }
                    if name == "x-close" && origin.y > strip_y
            )),
            "a cancelled child draws no stop"
        );
    }

    /// The row itself enters the child: a click on its description is the same
    /// gesture as the arrow at its end, so a plain (or double) click on the
    /// strip opens the child's view, the way grok's list does.
    #[test]
    fn a_click_on_the_panels_row_enters_the_child_view() {
        let Fixture { root, .. } = fixture();
        let state = root.state_rc();
        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        let commands = pane_commands(&mut root, &app);
        let row = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. }
                    if text.contains("audit the migration") =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the strip names the child's description");
        click(&mut root, &app, row + vec2f(4.0, 4.0));

        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent {
                conversation_id: "conv-child-1".to_string()
            },
            "a click on the strip's row enters the child's conversation"
        );
        let texts = pane_texts(&mut root, &app);
        assert!(
            texts
                .iter()
                .any(|text| text.contains("CHILD CONVERSATION MARKER")),
            "the child's own transcript is drawn, got {texts:?}"
        );
    }

    /// The open control in the strip enters the child's own conversation in the
    /// pane, whose surface draws no strip of its own.
    #[test]
    fn the_panels_open_enters_the_child_view() {
        let Fixture { root, .. } = fixture();
        let state = root.state_rc();
        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);
        let commands = pane_commands(&mut root, &app);
        let strip_y = run_origin(&commands, "Subagents").y;
        let open = icon_centre(&commands, "arrow-up", strip_y + 19.0);
        click(&mut root, &app, open);

        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent {
                conversation_id: "conv-child-1".to_string()
            },
            "the strip's open control enters the child's conversation"
        );
        let texts = pane_texts(&mut root, &app);
        assert!(
            texts
                .iter()
                .any(|text| text.contains("CHILD CONVERSATION MARKER")),
            "the child's own transcript is drawn, got {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Subagents"),
            "the child view draws no strip of its own, got {texts:?}"
        );
    }
