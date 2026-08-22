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
    ThreadReact { message_id: String, emoji: String },
    ThreadReplyTo { message_id: String },
    ThreadMarkRead { thread_id: String },
}

/// A reaction summary on a chat message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChatReaction {
    pub emoji: String,
    pub count: usize,
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

    pub fn code_block(lang: Option<String>, code: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::CodeBlock {
                lang,
                code: code.into(),
            },
        }
    }

    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Heading {
                level,
                text: text.into(),
            },
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
    pub author_name: Option<String>,
    pub timestamp: Option<String>,
    pub id: Option<String>,
    pub reactions: Vec<ChatReaction>,
    pub reply_to_id: Option<String>,
    pub reply_to_preview: Option<String>,
}

impl ChatMessage {
    pub fn new(role: ChatRole, fragments: Vec<ChatFragment>) -> Self {
        Self {
            role,
            fragments,
            author_name: None,
            timestamp: None,
            id: None,
            reactions: Vec::new(),
            reply_to_id: None,
            reply_to_preview: None,
        }
    }

    pub fn with_author_name(mut self, name: impl Into<String>) -> Self {
        self.author_name = Some(name.into());
        self
    }

    pub fn with_timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_reactions(mut self, reactions: Vec<ChatReaction>) -> Self {
        self.reactions = reactions;
        self
    }

    pub fn with_reply_to(
        mut self,
        id: impl Into<String>,
        preview: impl Into<String>,
    ) -> Self {
        self.reply_to_id = Some(id.into());
        self.reply_to_preview = Some(preview.into());
        self
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
        let author_name = message.author.participant_id().raw_id().to_string();
        let timestamp = message.created_at.to_rfc2822();
        let mut reactions: Vec<ChatReaction> = Vec::new();
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for r in &message.reactions {
            *counts.entry(r.emoji.clone()).or_insert(0) += 1;
        }
        for (emoji, count) in counts {
            reactions.push(ChatReaction { emoji, count });
        }
        let reply_to_id = message.reply_to.as_ref().map(|id| id.0.clone());
        let reply_to_preview = message
            .reply_to
            .as_ref()
            .and_then(|_| Some(String::new()));
        Self::from_markdown(role, message.content.clone())
            .with_author_name(author_name)
            .with_timestamp(timestamp)
            .with_id(message.id.0.clone())
            .with_reactions(reactions)
            .with_reply_to(reply_to_id.unwrap_or_default(), reply_to_preview.unwrap_or_default())
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
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    Heading {
        level: u8,
        text: String,
    },
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
