use std::sync::{Arc, Mutex};

use goble_terminal::blocks::BlockView;
use goble_terminal::{BlockOwner, HookEvent};

use super::*;

    #[test]
    fn registry_mode_defaults_to_shell() {
        let mut reg = TerminalRegistry::default();
        assert_eq!(reg.mode(1), TerminalMode::Shell);
        reg.set_mode(1, TerminalMode::Agent(TuiAgent::Codex));
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Codex));
        reg.drop_pane(1);
        assert_eq!(reg.mode(1), TerminalMode::Shell);
    }

    #[test]
    fn update_mode_from_input_detects_and_settles() {
        let mut reg = TerminalRegistry::default();
        reg.set_input(1, "claude".to_string());
        let agent = reg.update_mode_from_input(1);
        assert_eq!(agent, Some(TuiAgent::Claude));
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Claude));

        reg.set_input(1, "ls".to_string());
        assert_eq!(reg.update_mode_from_input(1), None);
        // A non-agent input never *downgrades* an already-agent pane; only a
        // detected agent switches the mode, so it stays agent-native here.
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Claude));
    }

    #[test]
    fn launch_agent_sets_agent_mode_even_for_unknown_commands() {
        let mut reg = TerminalRegistry::default();
        // No session spawned in a unit test, but mode is still set.
        reg.launch_agent(1, "codex");
        assert_eq!(reg.mode(1), TerminalMode::Agent(TuiAgent::Codex));
        reg.launch_agent(2, "some-custom-agent");
        assert_eq!(reg.mode(2), TerminalMode::Agent(TuiAgent::Other));
        // Blank is a no-op.
        reg.launch_agent(3, "  ");
        assert_eq!(reg.mode(3), TerminalMode::Shell);
    }

    #[test]
    fn a_pane_without_a_terminal_has_no_pane_session() {
        let reg = TerminalRegistry::default();
        assert!(reg.pane_session(1, "conv-1").is_none());
    }

    /// A pane only offers its own shell once that shell has bootstrapped the
    /// integration and can report a `Preexec`; a shell that cannot (anything
    /// outside bash/zsh, or one whose script failed to load) leaves the
    /// harness on the sandboxed runner instead of stranding a claim.
    #[test]
    fn a_pane_whose_shell_never_bootstrapped_has_no_pane_session() {
        let mut reg = TerminalRegistry::default();
        reg.sessions.insert(1, detached());

        assert!(
            reg.pane_session(1, "conv-1").is_none(),
            "a shell that cannot answer keeps the sandbox"
        );

        // The integration handshake is what makes the shell able to answer.
        run_hook(
            reg.sessions.get_mut(&1).expect("the session is there"),
            HookEvent::Bootstrapped(Default::default()),
        );
        assert!(
            reg.pane_session(1, "conv-1").is_some(),
            "a bootstrapped shell is the harness's route"
        );
    }

    // -- The owner in the two views (P6) ------------------------------------

    /// The terminal view and a conversation's agent view are two filters over
    /// one block list. After a mix of commands the user typed and one the agent
    /// ran, the terminal view shows all of them and the agent view shows only
    /// the agent's block.
    #[test]
    fn the_two_views_show_the_right_blocks_after_a_mixed_sequence() {
        let mut reg = TerminalRegistry::default();
        reg.sessions.insert(1, detached());
        let session = reg.sessions.get_mut(&1).expect("the session is there");
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(session, HookEvent::Bootstrapped(Default::default()));

        // The user types `ls`.
        run_hook(session, preexec("ls"));
        run_hook(session, HookEvent::CommandFinished(Default::default()));

        // The agent runs `echo hi` in the same shell, through P4's route.
        let answer = submit(session, "echo hi");
        session.pump();
        feed_bytes(session, b"echo hi\r\n");
        run_hook(session, preexec("echo hi"));
        feed_bytes(session, b"hi\r\n");
        run_hook(session, HookEvent::CommandFinished(Default::default()));
        assert_eq!(
            answer.blocking_recv().expect("the pane answers"),
            Ok("hi".to_string())
        );

        // The user types `pwd` afterwards.
        run_hook(session, preexec("pwd"));
        run_hook(session, HookEvent::CommandFinished(Default::default()));

        let terminal: Vec<String> = reg
            .visible_blocks(1, &BlockView::Terminal)
            .into_iter()
            .map(|block| block.command)
            .filter(|command| !command.is_empty())
            .collect();
        let agent = reg.visible_blocks(
            1,
            &BlockView::Agent {
                conversation_id: "conv-1".to_string(),
            },
        );

        // The user's `ls`, the agent's `echo hi` (a real block in the same
        // shell) and the user's `pwd`. The empty preamble/pending blocks are
        // not commands.
        assert_eq!(terminal, vec!["ls", "echo hi", "pwd"]);
        // Only the agent's command is in the conversation's view; the two the
        // user typed are not.
        assert_eq!(
            agent.iter().map(|b| b.command.as_str()).collect::<Vec<_>>(),
            vec!["echo hi"]
        );
        assert_eq!(
            agent[0].owner,
            BlockOwner::Agent {
                conversation_id: "conv-1".to_string(),
                call_id: "call-1".to_string(),
            }
        );
    }
