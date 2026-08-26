use crate::elements::terminal_block::TerminalData;

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

    pub fn terminal(data: TerminalData) -> Self {
        Self {
            kind: ChatFragmentKind::Terminal(data),
        }
    }
}

/// A single tool invocation recorded on an assistant message. `arguments` is the
/// JSON arguments the tool was called with, rendered alongside the name so the
/// user can see what the agent actually invoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    /// Parse tool-call metadata from the harness-produced `tool_calls` JSON column
    /// into renderable calls. Malformed or unknown JSON yields an empty list
    /// rather than failing the whole transcript.
    pub fn from_llm_json(json: &str) -> Vec<ToolCall> {
        #[derive(serde::Deserialize)]
        struct Raw {
            name: String,
            #[serde(default)]
            arguments: serde_json::Value,
        }
        serde_json::from_str::<Vec<Raw>>(json)
            .unwrap_or_default()
            .into_iter()
            .map(|raw| ToolCall {
                name: raw.name,
                arguments: serde_json::to_string(&raw.arguments).unwrap_or_default(),
            })
            .collect()
    }
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

/// The concrete content kind for a [`ChatFragment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatFragmentKind {
    Text(String),
    Bold(String),
    Italic(String),
    BoldItalic(String),
    Code(String),
    CodeBlock { lang: Option<String>, code: String },
    Heading { level: u8, text: String },
    Link { label: String, url: String },
    List { items: Vec<String>, ordered: bool },
    BlockQuote(String),
    LineBreak,
    Action { label: String, payload: ChatAction },
    Terminal(TerminalData),
}

/// The inline style of a span inside a paragraph block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineStyle {
    Plain,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link(String),
}

/// A single run of text with a uniform inline style, inside a paragraph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSpan {
    pub text: String,
    pub style: InlineStyle,
}

/// A block of chat content. A paragraph holds inline spans that flow and wrap
/// together; the other variants are stand-alone widgets drawn on their own row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatBlock {
    Paragraph(Vec<InlineSpan>),
    Heading { level: u8, text: String },
    CodeBlock { lang: Option<String>, code: String },
    List { items: Vec<String>, ordered: bool },
    BlockQuote(String),
    Action { label: String, payload: ChatAction },
    Terminal(TerminalData),
}

impl InlineSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::Plain,
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::Bold,
        }
    }

    pub fn italic(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::Italic,
        }
    }

    pub fn code(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: InlineStyle::Code,
        }
    }

    pub fn link(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: label.into(),
            style: InlineStyle::Link(url.into()),
        }
    }
}

fn inline_span(kind: &ChatFragmentKind) -> Option<InlineSpan> {
    match kind {
        ChatFragmentKind::Text(t) => Some(InlineSpan::plain(t.clone())),
        ChatFragmentKind::Bold(t) => Some(InlineSpan::bold(t.clone())),
        ChatFragmentKind::Italic(t) => Some(InlineSpan::italic(t.clone())),
        ChatFragmentKind::BoldItalic(t) => Some(InlineSpan {
            text: t.clone(),
            style: InlineStyle::BoldItalic,
        }),
        ChatFragmentKind::Code(t) => Some(InlineSpan::code(t.clone())),
        // Links stay interactive (rendered as an action chip), so they are not
        // folded into the inline flow of a paragraph.
        _ => None,
    }
}

/// Group a flat list of fragments into blocks: consecutive inline fragments
/// become a single paragraph, while structural fragments become their own block.
pub fn group_fragments_into_blocks(fragments: &[ChatFragment]) -> Vec<ChatBlock> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<InlineSpan> = Vec::new();

    let flush = |paragraph: &mut Vec<InlineSpan>, blocks: &mut Vec<ChatBlock>| {
        if !paragraph.is_empty() {
            blocks.push(ChatBlock::Paragraph(std::mem::take(paragraph)));
        }
    };

    for fragment in fragments {
        if let Some(span) = inline_span(&fragment.kind) {
            paragraph.push(span);
            continue;
        }
        flush(&mut paragraph, &mut blocks);
        match &fragment.kind {
            ChatFragmentKind::Heading { level, text } => {
                blocks.push(ChatBlock::Heading {
                    level: *level,
                    text: text.clone(),
                });
            }
            ChatFragmentKind::CodeBlock { lang, code } => {
                blocks.push(ChatBlock::CodeBlock {
                    lang: lang.clone(),
                    code: code.clone(),
                });
            }
            ChatFragmentKind::List { items, ordered } => {
                blocks.push(ChatBlock::List {
                    items: items.clone(),
                    ordered: *ordered,
                });
            }
            ChatFragmentKind::BlockQuote(text) => {
                blocks.push(ChatBlock::BlockQuote(text.clone()));
            }
            ChatFragmentKind::Action { label, payload } => {
                blocks.push(ChatBlock::Action {
                    label: label.clone(),
                    payload: payload.clone(),
                });
            }
            ChatFragmentKind::Link { label, url } => {
                blocks.push(ChatBlock::Action {
                    label: label.clone(),
                    payload: ChatAction::OpenUrl(url.clone()),
                });
            }
            ChatFragmentKind::Terminal(data) => {
                blocks.push(ChatBlock::Terminal(data.clone()));
            }
            // A line break separates paragraphs; it is represented by the flush
            // above and does not produce a block of its own.
            ChatFragmentKind::LineBreak => {}
            _ => {}
        }
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_inline_fragments_into_paragraph() {
        let fragments = vec![
            ChatFragment::text("Hello "),
            ChatFragment::bold("world"),
            ChatFragment::code("code"),
            ChatFragment::terminal(TerminalData::new(
                "cargo run",
                vec![crate::elements::terminal_block::TerminalLine::command("cargo run")],
            )),
            ChatFragment::text("Done"),
        ];
        let blocks = group_fragments_into_blocks(&fragments);
        assert_eq!(
            blocks,
            vec![
                ChatBlock::Paragraph(vec![
                    InlineSpan::plain("Hello "),
                    InlineSpan::bold("world"),
                    InlineSpan::code("code"),
                ]),
                ChatBlock::Terminal(TerminalData::new(
                    "cargo run",
                    vec![crate::elements::terminal_block::TerminalLine::command("cargo run")],
                )),
                ChatBlock::Paragraph(vec![InlineSpan::plain("Done")]),
            ]
        );
    }

    #[test]
    fn link_fragment_becomes_interactive_action() {
        let fragments = vec![ChatFragment::link("Goble", "https://goble.dev")];
        let blocks = group_fragments_into_blocks(&fragments);
        assert_eq!(
            blocks,
            vec![ChatBlock::Action {
                label: "Goble".to_string(),
                payload: ChatAction::OpenUrl("https://goble.dev".to_string()),
            }]
        );
    }

    #[test]
    fn heading_and_list_are_their_own_blocks() {
        let fragments = vec![
            ChatFragment::heading(1, "Title"),
            ChatFragment::list(vec!["a".to_string(), "b".to_string()], true),
        ];
        let blocks = group_fragments_into_blocks(&fragments);
        assert_eq!(
            blocks,
            vec![
                ChatBlock::Heading {
                    level: 1,
                    text: "Title".to_string(),
                },
                ChatBlock::List {
                    items: vec!["a".to_string(), "b".to_string()],
                    ordered: true,
                },
            ]
        );
    }

    #[test]
    fn tool_call_parses_harness_json() {
        let calls = ToolCall::from_llm_json(
            r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"}}]"#,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ls");
        assert!(calls[0].arguments.contains("/tmp"));

        // Malformed JSON is tolerated (empty list), never panics a transcript.
        assert!(ToolCall::from_llm_json("not json").is_empty());
        assert!(ToolCall::from_llm_json("").is_empty());
    }
}
