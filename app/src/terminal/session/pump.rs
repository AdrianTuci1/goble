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

    /// Take everything the screen produced since the last call and answer the
    /// questions it is waiting on.
    ///
    /// Called once per frame by the UI thread, which is where the font metrics
    /// the answers need are known. A program that asks the terminal a question
    /// (its colours, the text area size) waits for the reply, so this has to run
    /// even when nothing is being painted — the pane calls it every frame.
    pub fn pump(&mut self) {
        let (events, replies, hooks, osc, block_events) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let events = state.pump(&self.palette, self.cell);
            let replies = state.take_replies();
            (
                events,
                replies,
                state.take_hook_events(),
                state.take_osc_events(),
                state.take_block_events(),
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
            if self.hooks.len() == EVENT_BACKLOG {
                self.hooks.pop_front();
            }
            self.hooks.push_back(event);
        }
        // A block the agent claimed reaching a terminal state is that tool
        // call's result: its output and exit code answer the harness.
        for event in block_events {
            if let BlockEvent::ToolResult(result) = event {
                self.tool_results.push(result);
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
