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
