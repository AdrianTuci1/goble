use super::*;

impl UiState {
    /// Find the pane (if any) bound to a store conversation id. Used to route
    /// backend events (e.g. `chat:turn_finished`) to the pane that owns the turn.
    pub fn pane_id_for_conversation(&self, conversation_id: &str) -> Option<u64> {
        self.pane_sessions.iter().find_map(|(id, s)| {
            if s.conversation_id == conversation_id {
                Some(*id)
            } else {
                None
            }
        })
    }

    /// The store conversation id a pane is bound to. A pane with no explicit
    /// conversation lazily uses the currently selected conversation, which is
    /// how the initial single pane tracks the sidebar selection.
    pub fn pane_conversation_id(&self, pane_id: u64) -> Option<String> {
        self.pane_sessions
            .get(&pane_id)
            .map(|s| s.conversation_id.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if pane_id == self.active_pane_id {
                    self.selected_id.clone()
                } else {
                    None
                }
            })
    }

    /// The pane's own shell for an agent turn, when the pane has a terminal.
    ///
    /// A terminal pane's session is the harness's shell-tool route (P4): the
    /// agent's commands run in the user's own shell, visibly, instead of the
    /// sandbox. A pane that has never started a shell, or whose shell has not
    /// bootstrapped its integration (so it can never report a `Preexec`), has
    /// no session and `None` keeps the sandboxed runner.
    pub fn pane_session(
        &self,
        pane_id: u64,
        conversation_id: &str,
    ) -> Option<std::sync::Arc<dyn goble_core::harness::PaneSession>> {
        self.terminal.borrow().pane_session(pane_id, conversation_id)
    }

    /// The blocks a pane's terminal view draws, oldest first: the shell's own
    /// history. A command the agent ran is a real block in this list too, since
    /// it ran in the same shell.
    pub fn pane_terminal_view(&self, pane_id: u64) -> Vec<VisibleBlock> {
        self.terminal
            .borrow()
            .visible_blocks(pane_id, &BlockView::Terminal)
    }

    /// The blocks a conversation's agent view draws in `pane_id`, oldest first:
    /// the commands the agent ran for that conversation. A command the user
    /// typed has an owner that names no conversation, so it is not one of them.
    pub fn pane_agent_view(&self, pane_id: u64, conversation_id: &str) -> Vec<VisibleBlock> {
        self.terminal.borrow().visible_blocks(
            pane_id,
            &BlockView::Agent {
                conversation_id: conversation_id.to_string(),
            },
        )
    }

    /// The view this pane's terminal surface shows. A pane with no controls
    /// entry yet shows the shell's own history.
    pub fn pane_view(&self, pane_id: u64) -> BlockView {
        self.pane_controls(pane_id).view
    }

    /// Enter `conversation_id`'s agent view in `pane_id`: push the card that
    /// stands for the conversation into the pane's block list (so the terminal
    /// keeps a way back to it) and point the pane's filter at the conversation.
    ///
    /// Returns the card's block id, or `None` when the pane has no live
    /// session. Entering the same conversation again reuses its card, so a
    /// round trip through the terminal leaves one card, not a stack.
    pub fn enter_agent_view(
        &mut self,
        pane_id: u64,
        conversation_id: &str,
        label: &str,
    ) -> Option<BlockId> {
        let block =
            self.terminal
                .borrow_mut()
                .push_agent_view_block(pane_id, conversation_id, label);
        self.pane_controls_mut(pane_id).view = BlockView::Agent {
            conversation_id: conversation_id.to_string(),
        };
        block
    }

    /// Leave the agent view: the pane goes back to the shell's own history.
    /// The card the conversation left behind stays in the list.
    pub fn leave_agent_view(&mut self, pane_id: u64) {
        self.pane_controls_mut(pane_id).view = BlockView::Terminal;
    }

    /// The display name of a conversation, falling back to its id when the
    /// sidebar does not know it (a card naming a conversation from elsewhere).
    pub fn conversation_name(&self, conversation_id: &str) -> String {
        self.conversations
            .iter()
            .find(|c| c.id == conversation_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| conversation_id.to_string())
    }

    /// Whether `pane_id` owns a conversation of its own (rather than lazily
    /// following the sidebar selection, as the initial pane does). A pane with
    /// no conversation of its own — a freshly opened PTY workspace — gets one
    /// created on its first agent turn.
    pub fn pane_owns_conversation(&self, pane_id: u64) -> bool {
        self.pane_sessions
            .get(&pane_id)
            .map(|s| !s.conversation_id.is_empty())
            .unwrap_or(false)
    }

    /// Refresh one pane's runtime state (transcript + suspended ask) from the
    /// store conversation `conv`.
    pub(crate) fn refresh_pane(&mut self, pane_id: u64, conv: &str, desktop: &DesktopState) {
        let rt = self.pane_runtime.entry(pane_id).or_default();
        // A suspended ask persists in the store, so the inline card survives a
        // refresh; answering clears it (status becomes `answered`).
        rt.pending_ask = desktop
            .get_pending_ask(conv)
            .ok()
            .flatten()
            .and_then(|v| {
                let question = v.get("question")?.as_str()?.to_string();
                let quick: Vec<String> = v
                    .get("quick_replies")
                    .and_then(|q| q.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(AskUserUi::new(question, quick))
            });
        match desktop.list_chat_messages(conv) {
            Ok(msgs) => {
                // Only the rows whose content changed are re-parsed; the rest
                // are reused from the cache, so a frame with no new delta (or a
                // handful of deltas on one streaming row) costs no parsing for
                // the unchanged tail of the transcript.
                rt.messages = rt.parse_cache.resolve(&msgs);
            }
            Err(e) => {
                log::warn!("list_chat_messages({conv}): {e}");
            }
        }
        // The live reasoning steps are overlaid first, so they sit ahead of the
        // assistant message's prose; a running tool call is then overlaid after
        // the store read, visible even if its persisted row has not landed.
        overlay_reasoning(&mut rt.messages, &rt.reasoning);
        overlay_in_flight(&mut rt.messages, &rt.in_flight_tools);
        // The Local/Remote runtime decision is tracked per-conversation.
        if pane_id == self.active_pane_id {
            self.workspace_routing = desktop
                .get_chat_workspace_routing(conv)
                .ok()
                .flatten()
                .and_then(|s| routing_from_str(&s));
        }
    }
}
