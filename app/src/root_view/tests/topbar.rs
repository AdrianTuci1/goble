use super::*;

/// The agent-management control follows the pointer rules of every other
/// topbar button: a move over it only lights the control, and the release
/// inside it opens the panel.
#[test]
fn the_agent_management_control_opens_on_release_not_on_hover() {
    use goble_ui::elements::EventContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::Element;

    let (root, _bus, _dir) = root_with_bus();
    let state = root.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
    }
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(root);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let center = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                name, origin, size, ..
            } if name == "robot" && origin.y < crate::ui::shell::TOPBAR_HEIGHT => {
                Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
            }
            _ => None,
        })
        .expect("the agent management control");

    let mut ctx = EventContext;
    assert!(!root.dispatch_event(
        &DispatchedEvent::MouseMove { position: center },
        &mut ctx,
        &app,
    ));
    let _ = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    assert!(
        !state.borrow().agent_dashboard.open,
        "a pointer move over the control only lights it"
    );

    let _ = root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: center,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    let _ = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: center,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert!(
        state.borrow().agent_dashboard.open,
        "a release on the control opens the agent management panel"
    );
}

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
        s.right_sidebar_open = false;
        s.crons_open = false;
    }
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(root);

    // Nothing in flight: the topbar band draws no live cue.
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawIcon { origin, name, .. }
                if name == "diamond" && origin.y < crate::ui::shell::TOPBAR_HEIGHT
        )),
        "an idle topbar draws no live cue"
    );

    // One in-flight tool for the active pane: the cue shows the diamond
    // next to `1`. The event names the pane's own conversation, so it is
    // attributed to that pane rather than to whichever pane has focus.
    state.borrow_mut().record_pane_conversation(1, "conv-1");
    state.borrow_mut().apply_tool_event(&ToolCallEvent {
        chat_id: "conv-1".into(),
        id: "call_1".into(),
        name: "read_file".into(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
        status: ToolCallStatus::Running,
        result: None,
    });
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    // The pane's turn-status footer draws a diamond too, so the cue's own
    // icon is the one in the topbar band.
    let icon = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name, .. }
                if name == "diamond" && origin.y < crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the cue marks the count with the diamond icon");
    let origin = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. }
                if text == "1" && origin.y < crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the topbar cue draws the live count");

    // Click it, on the count and on the diamond beside it: the indicator is
    // not a hit target, so the events travel on unconsumed.
    let mut ctx = EventContext;
    for position in [origin + vec2f(2.0, 4.0), icon + vec2f(2.0, 4.0)] {
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
                "the topbar indicator takes no pointer event at {position:?}"
            );
        }
    }
}
