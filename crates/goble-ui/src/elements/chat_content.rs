/// The role of a chat message participant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatRole {
    User,
    Assistant,
}

/// An action that can be triggered from a chat fragment.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChatAction {
    OpenUrl(String),
    RunCommand(String),
    Custom(String),
}

/// A single piece of content inside a chat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatFragment {
    pub kind: ChatFragmentKind,
}

impl ChatFragment {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Text(text.into()),
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Bold(text.into()),
        }
    }

    pub fn italic(text: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Italic(text.into()),
        }
    }

    pub fn bold_italic(text: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::BoldItalic(text.into()),
        }
    }

    pub fn code(code: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Code(code.into()),
        }
    }

    pub fn link(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Link {
                label: label.into(),
                url: url.into(),
            },
        }
    }

    pub fn list(items: Vec<String>, ordered: bool) -> Self {
        Self {
            kind: ChatFragmentKind::List { items, ordered },
        }
    }

    pub fn block_quote(text: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::BlockQuote(text.into()),
        }
    }

    pub fn line_break() -> Self {
        Self {
            kind: ChatFragmentKind::LineBreak,
        }
    }

    pub fn action(label: impl Into<String>, payload: ChatAction) -> Self {
        Self {
            kind: ChatFragmentKind::Action {
                label: label.into(),
                payload,
            },
        }
    }
}

/// A chat message composed of one or more fragments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub fragments: Vec<ChatFragment>,
}

impl ChatMessage {
    pub fn new(role: ChatRole, fragments: Vec<ChatFragment>) -> Self {
        Self { role, fragments }
    }

    /// Build a chat message by parsing a Markdown string into fragments.
    pub fn from_markdown(role: ChatRole, text: impl Into<String>) -> Self {
        Self::new(role, crate::elements::markdown::parse_markdown(&text.into()))
    }

    /// Build a chat message from a service-layer thread message.
    pub fn from_thread_message(message: &goble_core::thread::ThreadMessage) -> Self {
        let role = if message.author.is_user() {
            ChatRole::User
        } else {
            ChatRole::Assistant
        };
        Self::from_markdown(role, message.content.clone())
    }
}

/// The concrete content kind for a [`ChatFragment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatFragmentKind {
    Text(String),
    Bold(String),
    Italic(String),
    BoldItalic(String),
    Code(String),
    Link {
        label: String,
        url: String,
    },
    List {
        items: Vec<String>,
        ordered: bool,
    },
    BlockQuote(String),
    LineBreak,
    Action {
        label: String,
        payload: ChatAction,
    },
}
