use super::fragment::ChatFragment;
use super::tool_call::ToolCall;

/// The role of a chat message participant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
}

/// An action that can be triggered from a chat fragment.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChatAction {
    OpenUrl(String),
    RunCommand(String),
    /// Open a sub-agent's own conversation. The payload is the child's
    /// conversation id. S5 drew the row's hit target; S6 delivers the view it
    /// enters — the child's own conversation, shown in the pane, Esc back.
    OpenSubAgent(String),
    Custom(String),
}

/// A chat message composed of one or more fragments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub fragments: Vec<ChatFragment>,
    pub author_name: Option<String>,
    pub timestamp: Option<String>,
    /// Tool invocations attached to an assistant message (the calls the agent
    /// made during this turn). Empty for user and tool-result rows.
    pub tool_calls: Vec<ToolCall>,
}

impl ChatMessage {
    pub fn new(role: ChatRole, fragments: Vec<ChatFragment>) -> Self {
        Self {
            role,
            fragments,
            author_name: None,
            timestamp: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub fn with_author_name(mut self, name: impl Into<String>) -> Self {
        self.author_name = Some(name.into());
        self
    }

    pub fn with_timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    /// Build a chat message by parsing a Markdown string into fragments.
    pub fn from_markdown(role: ChatRole, text: impl Into<String>) -> Self {
        Self::new(
            role,
            crate::elements::markdown::parse_markdown(&text.into()),
        )
    }

    /// Build a chat message from a service-layer thread message.
    pub fn from_thread_message(message: &goble_core::thread::ThreadMessage) -> Self {
        let role = if message.author.is_user() {
            ChatRole::User
        } else {
            ChatRole::Assistant
        };
        let author_name = message.author.participant_id().raw_id().to_string();
        let timestamp = message.created_at.to_rfc2822();
        Self::from_markdown(role, message.content.clone())
            .with_author_name(author_name)
            .with_timestamp(timestamp)
    }
}
