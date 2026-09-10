use crate::elements::terminal_block::TerminalData;
use goble_core::harness::ToolCallStatus;

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

/// A single list item: its checkbox state (only set for GFM task lists) and
/// the content it holds. Item content is itself a fragment sequence so a
/// nested paragraph, quote or list survives instead of being flattened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub checked: Option<bool>,
    pub content: Vec<ChatFragment>,
}

impl ListItem {
    pub fn new(content: Vec<ChatFragment>) -> Self {
        Self {
            checked: None,
            content,
        }
    }
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

    /// An ordered list carries its source start number; `None` means bulleted.
    pub fn list(items: Vec<ListItem>, start: Option<u64>) -> Self {
        Self {
            kind: ChatFragmentKind::List { items, start },
        }
    }

    pub fn block_quote(content: Vec<ChatFragment>) -> Self {
        Self {
            kind: ChatFragmentKind::BlockQuote(content),
        }
    }

    pub fn table(header: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self {
            kind: ChatFragmentKind::Table { header, rows },
        }
    }

    pub fn rule() -> Self {
        Self {
            kind: ChatFragmentKind::Rule,
        }
    }

    pub fn image(alt: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            kind: ChatFragmentKind::Image {
                alt: alt.into(),
                url: url.into(),
            },
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

    /// A model reasoning (thinking) step. `key` is the app-owned identity used
    /// to hold the row's collapsed/expanded state across frames; `mode` is the
    /// thinking mode label and `text` the step's accumulated text.
    pub fn reasoning(
        key: impl Into<String>,
        mode: impl Into<String>,
        text: impl Into<String>,
        done: bool,
    ) -> Self {
        Self {
            kind: ChatFragmentKind::Reasoning {
                key: key.into(),
                mode: mode.into(),
                text: text.into(),
                done,
            },
        }
    }
}

/// A single tool invocation recorded on an assistant message. `arguments` is the
/// JSON arguments the tool was called with, rendered alongside the name so the
/// user can see what the agent actually invoked. `status` and `result` are the
/// persisted lifecycle carrier: a re-read renders the terminal state, and an
/// in-flight `running` call is visible while it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: ToolCallStatus,
    pub result: Option<String>,
}

impl ToolCall {
    /// Parse tool-call metadata from the harness-produced `tool_calls` JSON column
    /// into renderable calls. Malformed or unknown JSON yields an empty list
    /// rather than failing the whole transcript. Rows written by older builds
    /// carry only `name`/`arguments`; `id`, `status` and `result` default.
    pub fn from_llm_json(json: &str) -> Vec<ToolCall> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default)]
            id: String,
            name: String,
            #[serde(default)]
            arguments: serde_json::Value,
            #[serde(default)]
            status: ToolCallStatus,
            #[serde(default)]
            result: Option<String>,
        }
        serde_json::from_str::<Vec<Raw>>(json)
            .unwrap_or_default()
            .into_iter()
            .map(|raw| ToolCall {
                id: raw.id,
                name: raw.name,
                arguments: serde_json::to_string(&raw.arguments).unwrap_or_default(),
                status: raw.status,
                result: raw.result,
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
    /// A list and its items. `start` is `Some(n)` for an ordered list whose
    /// first item is numbered `n`, and `None` for a bulleted list.
    List {
        items: Vec<ListItem>,
        start: Option<u64>,
    },
    /// Blockquote content as nested fragments, so paragraph breaks and nested
    /// blocks inside the quote are preserved.
    BlockQuote(Vec<ChatFragment>),
    /// A GFM table: one header row plus body rows.
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// A horizontal rule.
    Rule,
    /// An image, carried by its alt text and destination URL.
    Image { alt: String, url: String },
    LineBreak,
    Action { label: String, payload: ChatAction },
    Terminal(TerminalData),
    /// A model reasoning (thinking) step, rendered recessed and collapsed until
    /// the user expands it. `key` identifies the row's app-owned expand state.
    Reasoning {
        key: String,
        mode: String,
        text: String,
        done: bool,
    },
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
    List {
        items: Vec<ListItem>,
        start: Option<u64>,
    },
    /// A blockquote rendered as a nested stack of blocks, so it keeps its
    /// paragraph structure instead of collapsing to one line.
    BlockQuote(Vec<ChatBlock>),
    Table {
        header: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Rule,
    Image { alt: String, url: String },
    Action { label: String, payload: ChatAction },
    Terminal(TerminalData),
    /// A model reasoning step's own row: recessed (muted), collapsed until
    /// expanded. `key` identifies the row's app-owned expand state.
    Reasoning {
        key: String,
        mode: String,
        text: String,
        done: bool,
    },
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
        ChatFragmentKind::Link { label, url } => Some(InlineSpan::link(label.clone(), url.clone())),
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
            ChatFragmentKind::List { items, start } => {
                blocks.push(ChatBlock::List {
                    items: items.clone(),
                    start: *start,
                });
            }
            ChatFragmentKind::BlockQuote(content) => {
                blocks.push(ChatBlock::BlockQuote(group_fragments_into_blocks(content)));
            }
            ChatFragmentKind::Table { header, rows } => {
                blocks.push(ChatBlock::Table {
                    header: header.clone(),
                    rows: rows.clone(),
                });
            }
            ChatFragmentKind::Rule => blocks.push(ChatBlock::Rule),
            ChatFragmentKind::Image { alt, url } => {
                blocks.push(ChatBlock::Image {
                    alt: alt.clone(),
                    url: url.clone(),
                });
            }
            ChatFragmentKind::Action { label, payload } => {
                blocks.push(ChatBlock::Action {
                    label: label.clone(),
                    payload: payload.clone(),
                });
            }
            ChatFragmentKind::Terminal(data) => {
                blocks.push(ChatBlock::Terminal(data.clone()));
            }
            ChatFragmentKind::Reasoning {
                key,
                mode,
                text,
                done,
            } => {
                blocks.push(ChatBlock::Reasoning {
                    key: key.clone(),
                    mode: mode.clone(),
                    text: text.clone(),
                    done: *done,
                });
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
    fn link_fragment_stays_in_the_paragraph() {
        let fragments = vec![
            ChatFragment::text("see "),
            ChatFragment::link("Goble", "https://goble.dev"),
            ChatFragment::text(" for details"),
        ];
        let blocks = group_fragments_into_blocks(&fragments);
        assert_eq!(
            blocks,
            vec![ChatBlock::Paragraph(vec![
                InlineSpan::plain("see "),
                InlineSpan::link("Goble", "https://goble.dev"),
                InlineSpan::plain(" for details"),
            ])]
        );
    }

    #[test]
    fn heading_and_list_are_their_own_blocks() {
        let fragments = vec![
            ChatFragment::heading(1, "Title"),
            ChatFragment::list(
                vec![
                    ListItem::new(vec![ChatFragment::text("a")]),
                    ListItem::new(vec![ChatFragment::text("b")]),
                ],
                Some(3),
            ),
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
                    items: vec![
                        ListItem::new(vec![ChatFragment::text("a")]),
                        ListItem::new(vec![ChatFragment::text("b")]),
                    ],
                    start: Some(3),
                },
            ]
        );
    }

    #[test]
    fn reasoning_fragment_is_its_own_block() {
        let fragments = vec![
            ChatFragment::text("answer: "),
            ChatFragment::reasoning("c1:0", "contemplating", "weighing options", false),
        ];
        let blocks = group_fragments_into_blocks(&fragments);
        assert_eq!(
            blocks,
            vec![
                ChatBlock::Paragraph(vec![InlineSpan::plain("answer: ")]),
                ChatBlock::Reasoning {
                    key: "c1:0".to_string(),
                    mode: "contemplating".to_string(),
                    text: "weighing options".to_string(),
                    done: false,
                },
            ]
        );
    }

    #[test]
    fn tool_call_parses_harness_json() {
        let calls = ToolCall::from_llm_json(
            r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"},"status":"running"}]"#,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "ls");
        assert!(calls[0].arguments.contains("/tmp"));
        assert_eq!(calls[0].status, ToolCallStatus::Running);
        assert_eq!(calls[0].result, None);

        // Malformed JSON is tolerated (empty list), never panics a transcript.
        assert!(ToolCall::from_llm_json("not json").is_empty());
        assert!(ToolCall::from_llm_json("").is_empty());
    }

    #[test]
    fn tool_call_carries_status_and_result() {
        let calls = ToolCall::from_llm_json(
            r#"[{"id":"call_2","name":"credentials","arguments":{},"status":"finished","result":"no credentials stored"}]"#,
        );
        assert_eq!(calls[0].status, ToolCallStatus::Finished);
        assert_eq!(calls[0].result.as_deref(), Some("no credentials stored"));

        let failed = ToolCall::from_llm_json(
            r#"[{"id":"call_3","name":"no_such_tool","arguments":{},"status":"error","result":"unknown tool"}]"#,
        );
        assert_eq!(failed[0].status, ToolCallStatus::Error);
    }

    /// Rows persisted before the status carrier existed carry only
    /// `id`/`name`/`arguments`; they must still parse.
    #[test]
    fn tool_call_row_from_older_build_still_parses() {
        let calls =
            ToolCall::from_llm_json(r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"}}]"#);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "ls");
        assert_eq!(calls[0].status, ToolCallStatus::Pending);
        assert_eq!(calls[0].result, None);
    }
}
