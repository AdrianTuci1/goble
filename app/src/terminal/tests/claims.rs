use std::sync::{Arc, Mutex};
use std::time::Duration;

use goble_terminal::hooks::PreexecValue;
use goble_terminal::HookEvent;

use super::*;

    // -- The claim protocol (P2) --------------------------------------------

    #[test]
    fn a_claim_writes_the_command_to_the_pty() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));

        session.claim_command("git status", agent_owner()).unwrap();

        assert_eq!(&*captured.lock().unwrap(), b"git status\r");
        assert_eq!(
            session.pending_claim().map(|c| c.command.as_str()),
            Some("git status")
        );
    }

    #[test]
    fn a_claimed_command_resolves_on_its_own_preexec() {
        let mut session = detached();
        let owner = agent_owner();
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        session.claim_command("echo hi", owner.clone()).unwrap();

        run_hook(&mut session, preexec("echo hi"));

        assert!(session.pending_claim().is_none(), "the claim is consumed");
        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::Resolved {
                owner,
                command: "echo hi".to_string(),
            }]
        );
    }

    #[test]
    fn a_claim_that_never_gets_a_preexec_times_out() {
        let mut session = detached();
        let owner = agent_owner();
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        // A zero bounded wait, so the claim expires on the next pump without
        // the test sleeping.
        session.claim_timeout = Duration::ZERO;
        session.claim_command("echo hi", owner.clone()).unwrap();

        session.pump();

        assert!(session.pending_claim().is_none(), "the claim is failed");
        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::TimedOut {
                owner,
                expected: "echo hi".to_string(),
            }]
        );
        // A late `Preexec` cannot revive a claim that already failed.
        run_hook(&mut session, preexec("echo hi"));
        assert!(session.claim_outcomes().is_empty());
    }

    #[test]
    fn a_claim_is_refused_when_a_user_command_arrives_first() {
        let mut session = detached();
        let owner = agent_owner();
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        session.claim_command("echo agent", owner.clone()).unwrap();

        // The user hit enter before the agent's command reached the shell, so
        // the next `Preexec` is theirs.
        run_hook(&mut session, preexec("ls -la"));

        assert!(session.pending_claim().is_none(), "the claim is consumed");
        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::Refused {
                owner: owner.clone(),
                expected: "echo agent".to_string(),
                actual: "ls -la".to_string(),
            }]
        );

        // The agent's command running afterwards does not resurrect the claim:
        // the tool call already failed, and a guess is exactly what rule three
        // forbids.
        run_hook(&mut session, preexec("echo agent"));
        assert!(session.claim_outcomes().is_empty());
    }

    #[test]
    fn a_preexec_without_a_command_refuses_the_claim() {
        let mut session = detached();
        let owner = agent_owner();
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        session.claim_command("echo hi", owner.clone()).unwrap();

        // The shell reported a command start but not what it was: it cannot be
        // matched, so it is refused rather than attached blind.
        run_hook(
            &mut session,
            HookEvent::Preexec(PreexecValue { command: None }),
        );

        assert_eq!(
            session.claim_outcomes(),
            vec![ClaimOutcome::Refused {
                owner,
                expected: "echo hi".to_string(),
                actual: String::new(),
            }]
        );
    }

    #[test]
    fn a_second_claim_while_one_is_pending_is_refused() {
        let mut session = detached();
        run_hook(&mut session, HookEvent::Bootstrapped(Default::default()));
        session.claim_command("echo one", agent_owner()).unwrap();

        assert_eq!(
            session.claim_command("echo two", agent_owner()),
            Err(ClaimError::AlreadyPending)
        );
        assert_eq!(
            session.claim_command("   ", agent_owner()),
            Err(ClaimError::EmptyCommand)
        );
        assert_eq!(
            session.pending_claim().map(|c| c.command.as_str()),
            Some("echo one"),
            "the refused claims changed nothing"
        );
    }

    #[test]
    fn an_unclaimed_preexec_leaves_no_outcome() {
        let mut session = detached();
        run_hook(&mut session, preexec("ls"));
        assert!(session.claim_outcomes().is_empty());
        assert!(session.pending_claim().is_none());
    }

    /// A claim can only resolve if the shell reports a `Preexec`, which needs
    /// the bootstrapped integration. A session that never bootstrapped refuses
    /// the claim at once — nothing is written and nothing is left waiting for
    /// the bounded timeout.
    #[test]
    fn a_claim_on_a_shell_that_never_bootstrapped_fails_immediately() {
        let mut session = detached();
        let captured = Arc::new(Mutex::new(Vec::new()));
        session.writer = Some(Box::new(CaptureWriter(Arc::clone(&captured))));
        // The full wait would otherwise be spent before the claim failed.
        session.claim_timeout = Duration::from_secs(3600);

        assert_eq!(
            session.claim_command("echo hi", agent_owner()),
            Err(ClaimError::NotBootstrapped)
        );
        assert!(
            session.pending_claim().is_none(),
            "no claim is left waiting to time out"
        );
        assert!(
            captured.lock().unwrap().is_empty(),
            "the command is not written to a shell that cannot answer"
        );
    }
