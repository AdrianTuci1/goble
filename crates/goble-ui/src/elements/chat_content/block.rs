use crate::elements::terminal_block::TerminalData;
use super::fragment::{ChatFragment, ChatFragmentKind, ListItem};
use super::message::ChatAction;

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
