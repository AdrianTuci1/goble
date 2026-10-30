use std::sync::{Arc, Mutex};

use goble_terminal::HookEvent;

use super::*;

    // -- Routing the harness's shell tool through the pane (P4) --------------

    #[test]
    fn an_agent_command_runs_in_the_pane_and_returns_its_output() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        // The shell bootstrap: a claim needs a block to land on.
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));

        let answer = submit(&mut session, "echo hi");
        session.pump();
        assert_eq!(&*captured.lock().unwrap(), b"echo hi\r");
        assert!(session.pending_claim().is_some());

        // The shell echoes the line, reports the preexec, prints, and finishes.
        feed_bytes(&mut session, b"echo hi\r\n");
        run_hook(&mut session, preexec("echo hi"));
        feed_bytes(&mut session, b"hi\r\n");
        run_hook(&mut session, HookEvent::CommandFinished(Default::default()));

        assert_eq!(
            answer.blocking_recv().expect("the pane answers"),
            Ok("hi".to_string())
        );
    }

    /// The shell a harness turn routes its command through keeps being pumped
    /// even while the pane draws its agent view: nothing builds the pane's
    /// terminal in that frame, and pumping only painted shells left the call
    /// waiting until the tool timed out.
    ///
    /// The whole round trip runs through [`TerminalRegistry::pump_sessions`],
    /// the app's per-frame path: the request reaches the pty, the claim attaches
    /// to the `Preexec`, and the finished block's output is the tool result.
    #[test]
    fn a_pane_services_its_agent_command_without_painting_its_shell() {
        let mut registry = TerminalRegistry::default();
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        let answer = submit(&mut session, "echo hi");
        registry.sessions.insert(7, session);

        registry.pump_sessions();

        assert_eq!(
            &*captured.lock().unwrap(),
            b"echo hi\r",
            "the agent's command reached the pane's pty"
        );
        assert!(
            registry
                .sessions
                .get(&7)
                .is_some_and(|session| session.pending_claim().is_some()),
            "and the pane holds the claim that answers the call"
        );

        // What the shell writes back between frames: the echoed line, the
        // `Preexec` that proves the claimed command started, its output, and
        // the finished hook. Nothing here pumps a session directly.
        feed_to_registry(&mut registry, 7, b"echo hi\r\n");
        feed_to_registry(
            &mut registry,
            7,
            &goble_terminal::hooks::encode_hook(&preexec("echo hi")),
        );
        feed_to_registry(&mut registry, 7, b"hi\r\n");
        feed_to_registry(
            &mut registry,
            7,
            &goble_terminal::hooks::encode_hook(&HookEvent::CommandFinished(Default::default())),
        );

        assert_eq!(
            answer.blocking_recv().expect("the pane answers"),
            Ok("hi".to_string()),
            "the harness's shell call returns the block's output"
        );
    }

    /// Feed pty bytes into a registry-held session and let the frame's pump
    /// service it, exactly as [`TerminalRegistry::pump_sessions`] does in the app.
    fn feed_to_registry(registry: &mut TerminalRegistry, pane_id: u64, bytes: &[u8]) {
        {
            let session = registry.sessions.get(&pane_id).expect("the pane's session");
            session.state.lock().unwrap().feed(bytes);
        }
        registry.pump_sessions();
    }

    #[test]
    fn a_command_the_shell_never_ran_fails_the_agent_call() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));

        let answer = submit(&mut session, "echo agent");
        session.pump();

        // The user's own command runs first: rule three refuses the claim
        // rather than attaching it to the wrong block, so the tool call fails.
        run_hook(&mut session, preexec("ls -la"));

        let result = answer.blocking_recv().expect("the pane answers");
        assert!(result.is_err(), "a refused claim fails the tool call");
    }

    /// S3: what the agent's `run_command` gets on a pane whose shell is not at a
    /// prompt — the state an SSH-bound pane is in the whole time it is on the
    /// host, because the command its shell is running is the `ssh` line itself.
    /// A claim attaches to the block that is waiting for a command and this
    /// pane's block is executing, so the tool call fails up front instead of the
    /// line being written into the command that is already running.
    #[test]
    fn an_agent_command_is_refused_while_the_shell_is_inside_the_ssh_line() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        // The shell is inside `ssh web`: the `Preexec` arrived and no
        // `CommandFinished` can while the user is on the host.
        run_hook(&mut session, preexec("ssh web"));

        let answer = submit(&mut session, "echo hi");
        session.pump();

        assert_eq!(
            answer.blocking_recv().expect("the pane answers"),
            Err("the pane's shell is not ready to run an agent command".to_string()),
            "the pane refuses the claim rather than attaching it to the ssh line"
        );
        assert!(
            captured.lock().unwrap().is_empty(),
            "and writes nothing: the agent's command never reached the pty"
        );
    }
