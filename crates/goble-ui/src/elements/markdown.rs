use crate::elements::chat_content::ListItem;
use crate::elements::{ChatFragment, ChatFragmentKind};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Parse a Markdown string into a sequence of chat fragments.
///
/// Supported constructs:
/// - paragraphs and inline text
/// - bold (`**text**`), italic (`_text_`), bold+italic
/// - inline code and fenced code blocks (language is preserved)
/// - links `[label](url)`, kept inline in the paragraph they appear in
/// - images `![alt](url)`, carried as alt text plus destination
/// - unordered/ordered lists (ordered keeps the source start number), including
///   nested lists and GFM task lists
/// - blockquotes, keeping their inner paragraph structure
/// - GFM tables
/// - headings (H1–H6)
/// - horizontal rules
/// - line breaks
///
/// Raw HTML is neither rendered nor shown verbatim: the tags are stripped and
/// the surrounding text is kept.
pub fn parse_markdown(input: &str) -> Vec<ChatFragment> {
    let options = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let events: Vec<Event<'_>> = Parser::new_ext(input, options).collect();
    MarkdownParser::new(events).parse()
}

/// State for an image whose alt text is still being accumulated.
struct ImageState {
    url: String,
    alt: String,
}

struct MarkdownParser<'a> {
    events: Vec<Event<'a>>,
    pos: usize,
    // Active inline style counters.
    bold: usize,
    italic: usize,
    link_url: Option<String>,
    image: Option<ImageState>,
    pending: PendingText,
}

impl<'a> MarkdownParser<'a> {
    fn new(events: Vec<Event<'a>>) -> Self {
        Self {
            events,
            pos: 0,
            bold: 0,
            italic: 0,
            link_url: None,
            image: None,
            pending: PendingText::default(),
        }
    }

    fn parse(mut self) -> Vec<ChatFragment> {
        let mut fragments = self.parse_sequence(|_| false);
        merge_adjacent_text(&mut fragments);
        fragments
    }

    /// Parse events until the event matching `is_close` (which is consumed) or
    /// the end of the stream. Adjacent text fragments are merged.
    fn parse_sequence(&mut self, is_close: fn(&TagEnd) -> bool) -> Vec<ChatFragment> {
        let mut out = Vec::new();
        while self.pos < self.events.len() {
            if let Event::End(tag_end) = &self.events[self.pos] {
                if is_close(tag_end) {
                    self.pos += 1;
                    break;
                }
            }
            self.parse_event(&mut out);
        }
        self.flush(&mut out);
        merge_adjacent_text(&mut out);
        out
    }

    fn parse_event(&mut self, out: &mut Vec<ChatFragment>) {
        let event = self.events[self.pos].clone();
        self.pos += 1;
        match event {
            Event::Start(tag) => self.parse_start(tag, out),
            Event::End(tag_end) => self.parse_inline_end(tag_end, out),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => {
                if self.image.is_some() {
                    self.push_text(&code);
                } else {
                    self.flush(out);
                    out.push(ChatFragment::code(code.into_string()));
                }
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => {
                self.flush(out);
                out.push(ChatFragment::line_break());
            }
            Event::Rule => {
                self.flush(out);
                self.emit_block(out, vec![ChatFragment::rule()]);
            }
            // Task list markers are read by `parse_item`.
            Event::TaskListMarker(_) => {}
            // Raw HTML is stripped, never shown verbatim.
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }

    fn parse_start(&mut self, tag: Tag<'a>, out: &mut Vec<ChatFragment>) {
        // Any text still buffered belongs before this construct.
        self.flush(out);
        match tag {
            Tag::Paragraph => {
                let content = self.parse_sequence(|te| matches!(te, TagEnd::Paragraph));
                self.emit_block(out, content);
            }
            Tag::Heading { level, .. } => {
                let content = self.parse_sequence(|te| matches!(te, TagEnd::Heading(_)));
                let text = flatten_fragments(&content);
                if !text.is_empty() {
                    self.emit_block(out, vec![ChatFragment::heading(level as u8, text)]);
                }
            }
            Tag::BlockQuote(_) => {
                let content = self.parse_sequence(|te| matches!(te, TagEnd::BlockQuote(_)));
                if !content.is_empty() {
                    self.emit_block(out, vec![ChatFragment::block_quote(content)]);
                }
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                    CodeBlockKind::Indented => None,
                };
                let code = self.parse_code(|te| matches!(te, TagEnd::CodeBlock));
                if !code.is_empty() {
                    self.emit_block(out, vec![ChatFragment::code_block(lang, code)]);
                }
            }
            Tag::List(start) => {
                let list = self.parse_list(start);
                self.emit_block(out, vec![list]);
            }
            Tag::Table(_) => {
                let table = self.parse_table();
                self.emit_block(out, vec![table]);
            }
            // Flow tags consumed by `parse_list` / `parse_table`.
            Tag::Item | Tag::TableHead | Tag::TableRow | Tag::TableCell | Tag::HtmlBlock => {}
            Tag::Strong => self.bold += 1,
            Tag::Emphasis => self.italic += 1,
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some(ImageState {
                    url: dest_url.to_string(),
                    alt: String::new(),
                });
            }
            _ => {}
        }
    }

    fn parse_inline_end(&mut self, tag_end: TagEnd, out: &mut Vec<ChatFragment>) {
        match tag_end {
            TagEnd::Strong => {
                self.flush(out);
                self.bold = self.bold.saturating_sub(1);
            }
            TagEnd::Emphasis => {
                self.flush(out);
                self.italic = self.italic.saturating_sub(1);
            }
            TagEnd::Link => {
                self.flush(out);
                self.link_url = None;
            }
            TagEnd::Image => {
                if let Some(image) = self.image.take() {
                    out.push(ChatFragment::image(image.alt.trim(), image.url));
                }
            }
            // Block ends are consumed by the matching `parse_sequence`.
            _ => {}
        }
    }

    fn parse_list(&mut self, start: Option<u64>) -> ChatFragment {
        let mut items = Vec::new();
        while self.pos < self.events.len() {
            match &self.events[self.pos] {
                Event::End(TagEnd::List(_)) => {
                    self.pos += 1;
                    break;
                }
                Event::Start(Tag::Item) => {
                    self.pos += 1;
                    items.push(self.parse_item());
                }
                _ => self.pos += 1,
            }
        }
        ChatFragment::list(items, start)
    }

    fn parse_item(&mut self) -> ListItem {
        let checked = if let Some(Event::TaskListMarker(checked)) = self.events.get(self.pos) {
            let checked = *checked;
            self.pos += 1;
            Some(checked)
        } else {
            None
        };
        let content = self.parse_sequence(|te| matches!(te, TagEnd::Item));
        ListItem { checked, content }
    }

    fn parse_code(&mut self, is_close: fn(&TagEnd) -> bool) -> String {
        let mut code = String::new();
        while self.pos < self.events.len() {
            if let Event::End(tag_end) = &self.events[self.pos] {
                if is_close(tag_end) {
                    self.pos += 1;
                    break;
                }
            }
            if let Event::Text(text) = &self.events[self.pos] {
                code.push_str(text);
            }
            self.pos += 1;
        }
        code.trim().to_string()
    }

    fn parse_table(&mut self) -> ChatFragment {
        let mut header = Vec::new();
        let mut rows = Vec::new();
        while self.pos < self.events.len() {
            match &self.events[self.pos] {
                Event::End(TagEnd::Table) => {
                    self.pos += 1;
                    break;
                }
                // `TableHead` holds its cells directly; there is no header row tag.
                Event::Start(Tag::TableHead) => {
                    self.pos += 1;
                    while self.pos < self.events.len() {
                        match &self.events[self.pos] {
                            Event::End(TagEnd::TableHead) => {
                                self.pos += 1;
                                break;
                            }
                            Event::Start(Tag::TableCell) => {
                                self.pos += 1;
                                header.push(self.parse_cell());
                            }
                            _ => self.pos += 1,
                        }
                    }
                }
                Event::Start(Tag::TableRow) => {
                    self.pos += 1;
                    rows.push(self.parse_row());
                }
                _ => self.pos += 1,
            }
        }
        ChatFragment::table(header, rows)
    }

    fn parse_row(&mut self) -> Vec<String> {
        let mut cells = Vec::new();
        while self.pos < self.events.len() {
            match &self.events[self.pos] {
                Event::End(TagEnd::TableRow) => {
                    self.pos += 1;
                    break;
                }
                Event::Start(Tag::TableCell) => {
                    self.pos += 1;
                    cells.push(self.parse_cell());
                }
                _ => self.pos += 1,
            }
        }
        cells
    }

    fn parse_cell(&mut self) -> String {
        let mut text = String::new();
        while self.pos < self.events.len() {
            match &self.events[self.pos] {
                Event::End(TagEnd::TableCell) => {
                    self.pos += 1;
                    break;
                }
                Event::Text(t) => text.push_str(t),
                Event::Code(t) => text.push_str(t),
                Event::SoftBreak | Event::HardBreak => text.push(' '),
                _ => {}
            }
            self.pos += 1;
        }
        text.trim().to_string()
    }

    /// Emit a block-level run, separating it from a preceding block with a
    /// paragraph break so two adjacent paragraphs do not merge.
    fn emit_block(&self, out: &mut Vec<ChatFragment>, fragments: Vec<ChatFragment>) {
        if fragments.is_empty() {
            return;
        }
        if !out.is_empty()
            && !matches!(
                out.last().map(|f| &f.kind),
                Some(ChatFragmentKind::LineBreak)
            )
        {
            out.push(ChatFragment::line_break());
        }
        out.extend(fragments);
    }

    fn push_text(&mut self, text: &str) {
        if let Some(image) = self.image.as_mut() {
            image.alt.push_str(text);
            return;
        }
        append_text(
            &mut self.pending,
            text,
            self.bold,
            self.italic,
            self.link_url.as_deref(),
        );
    }

    fn flush(&mut self, out: &mut Vec<ChatFragment>) {
        if let Some(fragment) = take_pending(&mut self.pending) {
            out.push(fragment);
        }
    }
}

/// Concatenate the human-readable text of a fragment run (used for headings).
fn flatten_fragments(fragments: &[ChatFragment]) -> String {
    let mut text = String::new();
    for fragment in fragments {
        match &fragment.kind {
            ChatFragmentKind::Text(s)
            | ChatFragmentKind::Bold(s)
            | ChatFragmentKind::Italic(s)
            | ChatFragmentKind::BoldItalic(s)
            | ChatFragmentKind::Code(s) => text.push_str(s),
            ChatFragmentKind::Link { label, .. } => text.push_str(label),
            ChatFragmentKind::Image { alt, .. } => text.push_str(alt),
            _ => {}
        }
    }
    text.trim().to_string()
}

#[derive(Default, Clone)]
struct PendingText {
    text: String,
    bold: bool,
    italic: bool,
    link_url: Option<String>,
}

fn append_text(
    pending: &mut PendingText,
    text: &str,
    bold_count: usize,
    italic_count: usize,
    link_url: Option<&str>,
) {
    // Ignore leading pure-whitespace text when nothing is pending so that
    // spaces between differently-styled fragments do not become standalone
    // Text fragments.
    if pending.text.is_empty() && text.trim().is_empty() {
        return;
    }
    if pending.text.is_empty() {
        pending.bold = bold_count > 0;
        pending.italic = italic_count > 0;
        pending.link_url = link_url.map(|s| s.to_string());
    }
    pending.text.push_str(text);
}

fn take_pending(pending: &mut PendingText) -> Option<ChatFragment> {
    let text = std::mem::take(&mut pending.text).trim().to_string();
    let fragment = if text.is_empty() {
        None
    } else if let Some(url) = pending.link_url.take() {
        Some(ChatFragment::link(text, url))
    } else if pending.bold && pending.italic {
        Some(ChatFragment::bold_italic(text))
    } else if pending.bold {
        Some(ChatFragment::bold(text))
    } else if pending.italic {
        Some(ChatFragment::italic(text))
    } else {
        Some(ChatFragment::text(text))
    };
    pending.bold = false;
    pending.italic = false;
    pending.link_url = None;
    fragment
}

fn merge_adjacent_text(fragments: &mut Vec<ChatFragment>) {
    let mut merged: Vec<ChatFragment> = Vec::with_capacity(fragments.len());
    for fragment in fragments.drain(..) {
        if let Some(last) = merged.last_mut() {
            if let (ChatFragmentKind::Text(a), ChatFragmentKind::Text(b)) =
                (&last.kind, &fragment.kind)
            {
                let separator =
                    if a.ends_with(char::is_whitespace) || b.starts_with(char::is_whitespace) {
                        ""
                    } else {
                        " "
                    };
                let combined = format!("{}{}{}", a, separator, b);
                last.kind = ChatFragmentKind::Text(combined);
                continue;
            }
        }
        merged.push(fragment);
    }
    *fragments = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str) -> ListItem {
        ListItem::new(vec![ChatFragment::text(text)])
    }

    #[test]
    fn parses_plain_text() {
        let fragments = parse_markdown("hello world");
        assert_eq!(fragments, vec![ChatFragment::text("hello world")]);
    }

    #[test]
    fn separates_paragraphs() {
        let fragments = parse_markdown("one\n\ntwo");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::text("one"),
                ChatFragment::line_break(),
                ChatFragment::text("two"),
            ]
        );
    }

    #[test]
    fn parses_bold_and_italic() {
        let fragments = parse_markdown("**bold** _italic_");
        assert_eq!(
            fragments,
            vec![ChatFragment::bold("bold"), ChatFragment::italic("italic"),]
        );
    }

    #[test]
    fn parses_link() {
        let fragments = parse_markdown("[Goble](https://goble.dev)");
        assert_eq!(
            fragments,
            vec![ChatFragment::link("Goble", "https://goble.dev")]
        );
    }

    #[test]
    fn keeps_a_mid_sentence_link_in_the_paragraph() {
        let fragments = parse_markdown("see [Goble](https://goble.dev) for details");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::text("see"),
                ChatFragment::link("Goble", "https://goble.dev"),
                ChatFragment::text("for details"),
            ]
        );
    }

    #[test]
    fn parses_link_with_bold_label() {
        let fragments = parse_markdown("[**Goble**](https://goble.dev)");
        assert_eq!(
            fragments,
            vec![ChatFragment::link("Goble", "https://goble.dev")]
        );
    }

    #[test]
    fn parses_link_with_mixed_label() {
        let fragments = parse_markdown("[plain **bold**](https://goble.dev)");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::link("plain", "https://goble.dev"),
                ChatFragment::link("bold", "https://goble.dev"),
            ]
        );
    }

    #[test]
    fn parses_image() {
        let fragments = parse_markdown("![a diagram](https://example.com/d.png)");
        assert_eq!(
            fragments,
            vec![ChatFragment::image(
                "a diagram",
                "https://example.com/d.png"
            )]
        );
    }

    #[test]
    fn parses_inline_code() {
        let fragments = parse_markdown("run `cargo build`");
        assert_eq!(
            fragments,
            vec![ChatFragment::text("run"), ChatFragment::code("cargo build"),]
        );
    }

    #[test]
    fn parses_code_block() {
        let fragments = parse_markdown("```rust\nfn main() {}\n```");
        assert_eq!(
            fragments,
            vec![ChatFragment::code_block(
                Some("rust".to_string()),
                "fn main() {}"
            )]
        );
    }

    #[test]
    fn parses_unordered_list() {
        let fragments = parse_markdown("- one\n- two");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(vec![item("one"), item("two")], None)]
        );
    }

    #[test]
    fn parses_ordered_list() {
        let fragments = parse_markdown("1. first\n2. second");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(
                vec![item("first"), item("second")],
                Some(1)
            )]
        );
    }

    #[test]
    fn ordered_list_keeps_its_start_number() {
        let fragments = parse_markdown("3. third\n4. fourth");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(
                vec![item("third"), item("fourth")],
                Some(3)
            )]
        );
    }

    #[test]
    fn parses_list_with_styled_item() {
        let fragments = parse_markdown("- **one**\n- plain **bold**");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(
                vec![
                    ListItem::new(vec![ChatFragment::bold("one")]),
                    ListItem::new(vec![
                        ChatFragment::text("plain"),
                        ChatFragment::bold("bold"),
                    ]),
                ],
                None,
            )]
        );
    }

    #[test]
    fn parses_nested_list() {
        let fragments = parse_markdown("- one\n  - nested");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(
                vec![ListItem::new(vec![
                    ChatFragment::text("one"),
                    ChatFragment::line_break(),
                    ChatFragment::list(vec![item("nested")], None),
                ])],
                None,
            )]
        );
    }

    #[test]
    fn parses_task_list() {
        let fragments = parse_markdown("- [ ] todo\n- [x] done");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(
                vec![
                    ListItem {
                        checked: Some(false),
                        content: vec![ChatFragment::text("todo")],
                    },
                    ListItem {
                        checked: Some(true),
                        content: vec![ChatFragment::text("done")],
                    },
                ],
                None,
            )]
        );
    }

    #[test]
    fn parses_blockquote() {
        let fragments = parse_markdown("> quote");
        assert_eq!(
            fragments,
            vec![ChatFragment::block_quote(vec![ChatFragment::text("quote")])]
        );
    }

    #[test]
    fn blockquote_keeps_its_paragraphs() {
        let fragments = parse_markdown("> one\n>\n> two");
        assert_eq!(
            fragments,
            vec![ChatFragment::block_quote(vec![
                ChatFragment::text("one"),
                ChatFragment::line_break(),
                ChatFragment::text("two"),
            ])]
        );
    }

    #[test]
    fn parses_table() {
        let fragments = parse_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(
            fragments,
            vec![ChatFragment::table(
                vec!["a".to_string(), "b".to_string()],
                vec![vec!["1".to_string(), "2".to_string()]],
            )]
        );
    }

    #[test]
    fn parses_horizontal_rule() {
        let fragments = parse_markdown("---");
        assert_eq!(fragments, vec![ChatFragment::rule()]);
    }

    #[test]
    fn strips_raw_html() {
        let fragments = parse_markdown("before <span>inside</span> after");
        assert_eq!(fragments, vec![ChatFragment::text("before inside after")]);
        // An HTML block's tags are dropped with the block.
        assert!(parse_markdown("<div>\n\n</div>").is_empty());
    }

    #[test]
    fn parses_headings() {
        let fragments = parse_markdown("# H1\n## H2\n### H3");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::heading(1, "H1"),
                ChatFragment::line_break(),
                ChatFragment::heading(2, "H2"),
                ChatFragment::line_break(),
                ChatFragment::heading(3, "H3"),
            ]
        );
    }

    #[test]
    fn parses_hard_line_break_without_duplication() {
        let fragments = parse_markdown("**bold**\\\nmore");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::bold("bold"),
                ChatFragment::line_break(),
                ChatFragment::text("more"),
            ]
        );
    }
}
