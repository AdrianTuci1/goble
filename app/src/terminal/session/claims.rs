use std::time::Instant;

use goble_terminal::hooks::PreexecValue;
use goble_terminal::BlockOwner;

use crate::terminal::claim::{ClaimError, ClaimOutcome, PendingCommandClaim};

use super::{TerminalSession, EVENT_BACKLOG};

impl TerminalSession {
    /// Ask the pane's shell to run `command` on behalf of an agent turn.
    ///
    /// The command is written to the pty and held as a claim: the next `Preexec`
    /// the shell reports decides whether it was the command we asked for. The
    /// claim never attaches on a guess — a `Preexec` for a different command
    /// refuses it, and one that never comes times out (see [`ClaimOutcome`]).
    /// A shell whose integration has not bootstrapped cannot report a
    /// `Preexec` at all, so its claim is refused immediately.
    pub fn claim_command(&mut self, command: &str, owner: BlockOwner) -> Result<(), ClaimError> {
        let command = command.trim();
        if command.is_empty() {
            return Err(ClaimError::EmptyCommand);
        }
        if self.pending_claim.is_some() {
            return Err(ClaimError::AlreadyPending);
        }
        // A shell that has not bootstrapped never reports a `Preexec`, so the
        // claim could only time out; refuse it now, before it is written.
        if !self.is_bootstrapped() {
            return Err(ClaimError::NotBootstrapped);
        }
        // Enter, as the terminal sees it: a carriage return. A program in raw
        // mode does not read a line feed as "run this".
        let mut bytes = command.as_bytes().to_vec();
        bytes.push(b'\r');
        self.write(&bytes);
        self.pending_claim = Some(PendingCommandClaim {
            owner,
            command: command.to_string(),
            deadline: Instant::now() + self.claim_timeout,
        });
        Ok(())
    }

    /// The claim waiting for its `Preexec`, if any.
    pub fn pending_claim(&self) -> Option<&PendingCommandClaim> {
        self.pending_claim.as_ref()
    }

    /// Take the claim verdicts seen since the last call, oldest first.
    pub fn claim_outcomes(&mut self) -> Vec<ClaimOutcome> {
        self.claim_outcomes.drain(..).collect()
    }

    /// Resolve the pending claim against one `Preexec`.
    ///
    /// A `Preexec` with no claim is the user's command and touches nothing.
    /// With a claim, only the same command line resolves it; anything else —
    /// including a `Preexec` that does not report a command at all — refuses
    /// the claim, because attaching it to the wrong block is worse than failing
    /// the tool call.
    pub(super) fn observe_preexec(&mut self, value: &PreexecValue) {
        let Some(claim) = self.pending_claim.take() else {
            return;
        };
        let actual = value.command.clone().unwrap_or_default();
        let outcome = if actual.trim() == claim.command {
            ClaimOutcome::Resolved {
                owner: claim.owner,
                command: claim.command,
            }
        } else {
            ClaimOutcome::Refused {
                owner: claim.owner,
                expected: claim.command,
                actual,
            }
        };
        self.push_claim_outcome(outcome);
    }

    /// Fail a claim that outlived its bounded wait.
    pub(super) fn expire_claim(&mut self) {
        let Some(claim) = self.pending_claim.as_ref() else {
            return;
        };
        if Instant::now() < claim.deadline {
            return;
        }
        let claim = self.pending_claim.take().expect("checked above");
        self.push_claim_outcome(ClaimOutcome::TimedOut {
            owner: claim.owner,
            expected: claim.command,
        });
    }

    fn push_claim_outcome(&mut self, outcome: ClaimOutcome) {
        if self.claim_outcomes.len() == EVENT_BACKLOG {
            self.claim_outcomes.pop_front();
        }
        self.claim_outcomes.push_back(outcome);
    }
}
