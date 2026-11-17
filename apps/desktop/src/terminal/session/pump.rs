use goble_terminal::{BlockEvent, BlockOwner, HookEvent, OscEvent};

use crate::terminal::claim::{owner_call_id, tool_result_text, ClaimOutcome};

use super::{TerminalSession, EVENT_BACKLOG};

impl TerminalSession {
    /// Write bytes to the shell's stdin. Non-blocking for the UI thread.
    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Write bytes after `delay`, drained by the next [`Self::pump`] that comes
    /// due. A CLI agent that treats a prompt and the Enter after it as one paste
    /// needs the two as separate reads; waiting on the UI thread for that would
    /// stall every pane.
    pub fn write_after(&mut self, delay: std::time::Duration, bytes: Vec<u8>) {
        self.pending_writes
            .push((std::time::Instant::now() + delay, bytes));
    }

    /// Feed a shell-integration hook into the screen and pump it, exactly as
    /// the reader thread does with the bytes the shell wrote. A session with no
    /// pty behind it — a test's — has no other way to report one.
    #[cfg(test)]
    pub(crate) fn feed_hook(&mut self, event: HookEvent) {
        if let Ok(mut state) = self.state.lock() {
            state.feed(&goble_terminal::hooks::encode_hook(&event));
        }
        self.pump();
    }

    fn flush_due_writes(&mut self) {
        if self.pending_writes.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        let mut due = Vec::new();
        self.pending_writes.retain(|(at, bytes)| {
            if *at <= now {
                due.push(bytes.clone());
                false
            } else {
                true
            }
        });
        for bytes in due {
            self.write(&bytes);
        }
    }

    /// Take everything the screen produced since the last call and answer the
    /// questions it is waiting on.
    ///
    /// Called once per frame by the UI thread, which is where the font metrics
    /// the answers need are known. A program that asks the terminal a question
    /// (its colours, the text area size) waits for the reply, so this has to run
    /// even when nothing is being painted — the pane calls it every frame.
    pub fn pump(&mut self) {
        self.flush_due_writes();
        let (events, replies, hooks, osc, block_events, finishes) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let events = state.pump(&self.palette, self.cell);
            let replies = state.take_replies();
            let block_events = state.take_block_events();
            // What each finishing block was, read now: the block is frozen but
            // still in the list, and the event itself carries only its id.
            let finishes: Vec<Option<crate::emulator::FinishedBlock>> = block_events
                .iter()
                .map(|event| match event {
                    BlockEvent::BlockFinished { id, .. } => state.finished_block(*id),
                    _ => None,
                })
                .collect();
            (
                events,
                replies,
                state.take_hook_events(),
                state.take_osc_events(),
                block_events,
                finishes,
            )
        };

        for event in events {
            match event {
                goble_terminal::ScreenEvent::Title(title) => self.title = Some(title),
                goble_terminal::ScreenEvent::ResetTitle => self.title = None,
                goble_terminal::ScreenEvent::Bell => self.bell = true,
                _ => {}
            }
        }

        for event in hooks {
            // The claim is resolved (or refused) as the hooks arrive, so the
            // pane never has to hand its hook stream to somebody else first.
            if let HookEvent::Preexec(value) = &event {
                self.observe_preexec(value);
            }
            // The shell's own `cd` is what moves a pane to another directory,
            // and `cd` prints nothing: the hook channel is where the move shows
            // up (OSC 7 is the other carrier, folded in below).
            if let Some((path, start)) = hook_cwd(&event) {
                self.record_reported_cwd(path, start);
            }
            if self.hooks.len() == EVENT_BACKLOG {
                self.hooks.pop_front();
            }
            self.hooks.push_back(event);
        }
        // A block the agent claimed reaching a terminal state is that tool
        // call's result: its output and exit code answer the harness. A block
        // that ran a command is also the telemetry outcome for that command.
        for (event, finish) in block_events.into_iter().zip(finishes) {
            match event {
                BlockEvent::ToolResult(result) => self.tool_results.push(result),
                BlockEvent::BlockFinished {
                    exit_code, failed, ..
                } => {
                    let Some(finish) = finish.filter(|finish| finish.ran_a_command) else {
                        continue;
                    };
                    let outcome = if failed || exit_code != 0 {
                        goble_telemetry::Outcome::Error
                    } else {
                        goble_telemetry::Outcome::Success
                    };
                    let by_user = matches!(finish.owner, BlockOwner::User);
                    crate::telemetry::terminal_command_finished(outcome, finish.duration, by_user);
                }
                _ => {}
            }
        }
        // A claim the shell never answered is failed on the frame it expires
        // rather than left pending forever; the pane pumps every frame.
        self.expire_claim();
        for event in osc {
            if let OscEvent::WorkingDirectory(path) = &event {
                self.cwd = Some(path.clone());
            }
            if self.osc.len() == EVENT_BACKLOG {
                self.osc.pop_front();
            }
            self.osc.push_back(event);
        }

        if !replies.is_empty() {
            self.write(&replies);
        }

        // The harness's shell tool rides this same frame: submit, claim, and
        // hand back the block's result once the hooks have produced it.
        self.service_pane_commands();
    }

    /// Carry the harness's shell tool through this pane (P4).
    ///
    /// One submitted command is claimed for its tool call and written to the
    /// pty; a claim the shell refuses, or one that outlives the bounded wait,
    /// fails the tool call, and a claimed block's tool result answers it with
    /// its output and exit code. An unmapped result (a stale call) is dropped.
    fn service_pane_commands(&mut self) {
        let request = self
            .commands
            .lock()
            .ok()
            .and_then(|mut commands| commands.request.take());
        if let Some(request) = request {
            let owner = BlockOwner::Agent {
                conversation_id: request.conversation_id.clone(),
                call_id: request.call_id.clone(),
            };
            if !self.arm_block_claim(&owner) {
                let _ = request.reply.send(Err(
                    "the pane's shell is not ready to run an agent command".to_string(),
                ));
            } else if let Err(error) = self.claim_command(&request.line, owner) {
                let _ = request.reply.send(Err(format!(
                    "the pane could not claim the command: {error:?}"
                )));
            } else {
                self.awaiting.insert(request.call_id, request.reply);
            }
        }

        for outcome in self.claim_outcomes() {
            let failure = match &outcome {
                ClaimOutcome::Resolved { .. } => None,
                ClaimOutcome::Refused {
                    owner,
                    expected,
                    actual,
                } => Some((
                    owner_call_id(owner),
                    format!("the pane's shell ran `{actual}` first, not `{expected}`"),
                )),
                ClaimOutcome::TimedOut { owner, expected } => Some((
                    owner_call_id(owner),
                    format!("the pane's shell did not run `{expected}` in time"),
                )),
            };
            match failure {
                // A verdict for one of our tool calls fails it; the pane's own
                // record of the rest is left intact for whoever reads it.
                Some((call_id, message)) => match self.awaiting.remove(&call_id) {
                    Some(reply) => {
                        let _ = reply.send(Err(message));
                    }
                    None => self.claim_outcomes.push_back(outcome),
                },
                None => self.claim_outcomes.push_back(outcome),
            }
        }

        for result in std::mem::take(&mut self.tool_results) {
            let Some(reply) = self.awaiting.remove(&result.call_id) else {
                continue;
            };
            let _ = reply.send(tool_result_text(&result));
        }
    }

    /// Aim the block list's next `Preexec` at the active block, so the command
    /// the agent's tool call runs becomes a block that call owns.
    fn arm_block_claim(&mut self, owner: &BlockOwner) -> bool {
        match self.state.lock() {
            Ok(mut state) => state.claim_active_block(owner.clone()),
            Err(_) => false,
        }
    }
}

/// The working directory a hook carries, if it carries one, and whether it is
/// where the shell started rather than a directory its prompt reported: every
/// `Precmd` reports where its prompt is, and `InitShell` reports where the shell
/// was put. A blank value is not a directory, so it is dropped rather than
/// written.
fn hook_cwd(event: &HookEvent) -> Option<(String, bool)> {
    let (reported, start) = match event {
        HookEvent::Precmd(value) => (value.pwd.as_deref(), false),
        HookEvent::InitShell(value) => (value.cwd.as_deref(), true),
        _ => return None,
    };
    let reported = reported?.trim();
    (!reported.is_empty()).then(|| (reported.to_string(), start))
}
