use super::*;

/// The whole-surface filter chord. Cmd/Ctrl+F belongs to the panes, and the root
/// raises it for the one kind of pane that cannot answer it itself: an agent
/// pane's transcript, whose terminal blocks have no pty behind them to hand the
/// key to. A shell pane keeps the chord, because it also owns Cmd+Shift+F (the
/// hovered block's own filter) and the rule that a full-screen program has no
/// block list to filter.
#[test]
fn cmd_f_raises_the_transcript_filter_of_an_agent_pane_and_leaves_a_shell_pane_its_own() {
    use goble_terminal::blocks::BlockView;
    use goble_ui::elements::{AppContext, Element};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::vec2f;
    use goble_ui::test_util::render_element;

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    ));
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    let pane_id = {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.active_pane_id
    };
    let mut root: Box<dyn Element> = Box::new(view);
    // One frame first: the root dispatches its chords through the actions the
    // last rebuild built.
    let _ = render_element(&mut root, vec2f(1200.0, 800.0), &app);

    let cmd_f = DispatchedEvent::KeyDown {
        key: "f".to_string(),
        modifiers: ModifiersState {
            command: true,
            ..ModifiersState::default()
        },
    };
    let mut ctx = EventContext::default();

    assert!(
        !root.dispatch_event(&cmd_f, &mut ctx, &app),
        "the root leaves a shell pane's filter chord to the pane"
    );

    // Onto the pane's agent surface, through the one writer that decides it:
    // the harness switch and the view filter are written together there
    // (`PaneControls::surface`, U1), so a pane is an agent pane exactly when
    // that switch is on. Writing the view filter alone used to stand for the
    // same thing; it does not any more, because a pane whose switch is off is a
    // shell whatever its filter was left on.
    state
        .borrow_mut()
        .enter_agent_view(pane_id, "c1", "Parent conversation");
    assert!(
        matches!(state.borrow().pane_view(pane_id), BlockView::Agent { .. }),
        "the pane is on its agent surface"
    );
    let _ = render_element(&mut root, vec2f(1200.0, 800.0), &app);
    assert!(
        root.dispatch_event(&cmd_f, &mut ctx, &app),
        "the root takes Cmd+F for an agent pane"
    );
    let filter = state
        .borrow()
        .terminal_global_filters
        .get(&pane_id)
        .cloned()
        .expect("the pane's filter is seeded");
    assert!(filter.is_open(), "Cmd+F raises the transcript filter");
    assert!(
        *filter.focused.borrow(),
        "and puts the caret in its field, which is what types into it"
    );

    let _ = render_element(&mut root, vec2f(1200.0, 800.0), &app);
    assert!(root.dispatch_event(&cmd_f, &mut ctx, &app));
    assert!(!filter.is_open(), "a second Cmd+F puts the bar away again");
}
