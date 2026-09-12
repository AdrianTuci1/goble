use std::collections::BTreeSet;

use super::state::BlockView;

/// Which views a block belongs to.
///
/// The terminal and each agent view are filters over one list, not separate
/// histories: entering a conversation hides the terminal's blocks and shows the
/// ones associated with it. A block can belong to both — a command typed inside
/// a conversation is a terminal block that the conversation also shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockVisibility {
    terminal: bool,
    conversations: BTreeSet<String>,
}

impl Default for BlockVisibility {
    /// A block starts as shell output: the terminal shows it.
    fn default() -> Self {
        Self::terminal()
    }
}

impl BlockVisibility {
    /// Visible in the terminal view only; this is what a shell block starts as.
    pub fn terminal() -> Self {
        Self {
            terminal: true,
            conversations: BTreeSet::new(),
        }
    }

    /// Belongs to one conversation's agent view and not to the terminal view.
    pub fn agent(conversation_id: impl Into<String>) -> Self {
        let mut conversations = BTreeSet::new();
        conversations.insert(conversation_id.into());
        Self {
            terminal: false,
            conversations,
        }
    }

    pub fn is_in_terminal(&self) -> bool {
        self.terminal
    }

    pub fn conversations(&self) -> impl Iterator<Item = &str> {
        self.conversations.iter().map(String::as_str)
    }

    pub fn is_in_conversation(&self, conversation_id: &str) -> bool {
        self.conversations.contains(conversation_id)
    }

    pub fn is_visible_in(&self, view: &BlockView) -> bool {
        match view {
            BlockView::Terminal => self.terminal,
            BlockView::Agent { conversation_id } => self.is_in_conversation(conversation_id),
        }
    }

    pub(super) fn associate(&mut self, conversation_id: &str) -> bool {
        self.conversations.insert(conversation_id.to_string())
    }
}
