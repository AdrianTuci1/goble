use crate::color::ColorU;
use crate::elements::terminal_block::TerminalData;
use super::message::ChatAction;

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
    /// A colour the source asked for with a `{color:…}` marker. `None` keeps
    /// the theme's own colour for the run, which is what every fragment that
    /// did not name one carries.
    pub color: Option<ColorU>,
}

impl ChatFragment {
    fn of(kind: ChatFragmentKind) -> Self {
        Self { kind, color: None }
    }

    /// Paint this run in an explicit colour instead of its theme colour. The
    /// markdown parser is the caller: a `{color:#rrggbb}` marker wraps the run
    /// it opens.
    pub fn with_color(mut self, color: ColorU) -> Self {
        self.color = Some(color);
        self
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Text(text.into()))
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Bold(text.into()))
    }

    pub fn italic(text: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Italic(text.into()))
    }

    pub fn bold_italic(text: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::BoldItalic(text.into()))
    }

    pub fn code(code: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Code(code.into()))
    }

    pub fn code_block(lang: Option<String>, code: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::CodeBlock {
            lang,
            code: code.into(),
        })
    }

    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Heading {
            level,
            text: text.into(),
        })
    }

    pub fn link(label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Link {
            label: label.into(),
            url: url.into(),
        })
    }

    /// An ordered list carries its source start number; `None` means bulleted.
    pub fn list(items: Vec<ListItem>, start: Option<u64>) -> Self {
        Self::of(ChatFragmentKind::List { items, start })
    }

    pub fn block_quote(content: Vec<ChatFragment>) -> Self {
        Self::of(ChatFragmentKind::BlockQuote(content))
    }

    pub fn table(header: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self::of(ChatFragmentKind::Table { header, rows })
    }

    pub fn rule() -> Self {
        Self::of(ChatFragmentKind::Rule)
    }

    pub fn image(alt: impl Into<String>, url: impl Into<String>) -> Self {
        Self::of(ChatFragmentKind::Image {
            alt: alt.into(),
            url: url.into(),
        })
    }

    pub fn line_break() -> Self {
        Self::of(ChatFragmentKind::LineBreak)
    }

    pub fn action(label: impl Into<String>, payload: ChatAction) -> Self {
        Self::of(ChatFragmentKind::Action {
            label: label.into(),
            payload,
        })
    }

    pub fn terminal(data: TerminalData) -> Self {
        Self::of(ChatFragmentKind::Terminal(data))
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
        Self::of(ChatFragmentKind::Reasoning {
            key: key.into(),
            mode: mode.into(),
            text: text.into(),
            done,
        })
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
