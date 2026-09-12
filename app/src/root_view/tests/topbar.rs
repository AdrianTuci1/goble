use super::*;

    /// C3's acceptance: the topbar cue reads C1's live count and is absent at
    /// zero. It is information only: a click on it is not a hit target.
    #[test]
    fn the_topbar_cue_reads_the_live_count_and_is_inert() {
        use goble_core::harness::ToolCallStatus;
        use goble_desktop_service::ToolCallEvent;
        use goble_ui::elements::EventContext;
        use goble_ui::geometry::vec2f;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;
        use goble_ui::Element;

        let (root, _bus, _dir) = root_with_bus();
        let state = root.state_rc();
        // No first-run overlay may sit above the bar and swallow the click.
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
        }
        let app = AppContext::default();
        let mut root: Box<dyn Element> = Box::new(root);

        // Nothing in flight: the topbar draws no live cue.
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        assert!(
            !commands.iter().any(|c| matches!(
                c,
                RenderCommand::DrawText { text, .. } if text.starts_with("◆ ")
            )),
            "an idle topbar draws no live cue"
        );

        // One in-flight tool for the active pane: the cue shows `◆ 1`.
        state.borrow_mut().apply_tool_event(&ToolCallEvent {
            chat_id: "conv-1".into(),
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
            status: ToolCallStatus::Running,
            result: None,
        });
        let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
        let origin = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::DrawText { origin, text, .. } if text == "◆ 1" => Some(*origin),
                _ => None,
            })
            .expect("the topbar cue draws the live count");

        // Click it: the indicator is not a hit target, so the event travels on
        // unconsumed.
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
            assert!(
                !root.dispatch_event(&event, &mut ctx, &app),
                "the topbar indicator takes no pointer event"
            );
        }
    }
