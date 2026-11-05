use crate::color::ColorU;
use crate::elements::chat_content::ListItem;
use crate::elements::{ChatFragment, ChatFragmentKind};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Parse a Markdown string into a sequence of chat fragments.
///
/// Supported constructs:
/// - paragraphs and inline text
/// - bold (`**text**`), italic (`_text_`), bold+italic
/// - an explicit run colour: `{color:#rrggbb}text{/color}`, or a named colour
///   (`red`, `orange`, `yellow`, `green`, `blue`, `purple`, `cyan`, `gray`,
///   `white`, `black`). Colour nests over every other inline style.
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
    /// The colours the source has opened, innermost last. A `{color:…}` marker
    /// pushes and `{/color}` pops, so a run inside two markers takes the inner
    /// colour and the outer one resumes when it closes.
    colors: Vec<ColorU>,
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
            colors: Vec::new(),
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
            Event::Text(text) => self.push_text_to(&text, out),
            Event::Code(code) => {
                if self.image.is_some() {
                    self.push_text_to(&code, out);
                } else {
                    self.flush(out);
                    out.push(ChatFragment::code(code.into_string()));
                }
            }
            Event::SoftBreak => self.push_text_to(" ", out),
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

    /// Append a text run, honouring the `{color:…}` markers it carries.
    ///
    /// A marker splits the run: the text before it is flushed in the colour in
    /// force, the marker pushes or pops the colour stack, and the text after it
    /// starts a new run. A marker that names no known colour, and a closing
    /// marker with nothing open, stay literal text.
    fn push_text_to(&mut self, text: &str, out: &mut Vec<ChatFragment>) {
        if let Some(image) = self.image.as_mut() {
            image.alt.push_str(text);
            return;
        }
        let bold = self.bold;
        let italic = self.italic;
        let link_url = self.link_url.clone();
        let mut rest = text;
        loop {
            let open = rest.find("{color:");
            let close = rest.find("{/color}");
            let (index, closing) = match (open, close) {
                (Some(open), Some(close)) if close < open => (close, true),
                (Some(open), _) => (open, false),
                (None, Some(close)) => (close, true),
                (None, None) => {
                    append_text(
                        &mut self.pending,
                        rest,
                        bold,
                        italic,
                        link_url.as_deref(),
                        self.colors.last().copied(),
                    );
                    break;
                }
            };
            let before = &rest[..index];
            if !before.is_empty() {
                append_text(
                    &mut self.pending,
                    before,
                    bold,
                    italic,
                    link_url.as_deref(),
                    self.colors.last().copied(),
                );
            }
            let marker = &rest[index..];
            if closing {
                if self.colors.is_empty() {
                    // A close with nothing open is text: keep it in the run it
                    // appears in instead of starting a new one.
                    append_text(
                        &mut self.pending,
                        "{/color}",
                        bold,
                        italic,
                        link_url.as_deref(),
                        None,
                    );
                } else {
                    self.flush(out);
                    self.colors.pop();
                }
                rest = &marker["{/color}".len()..];
                continue;
            }
            let Some(end) = marker.find('}') else {
                append_text(
                    &mut self.pending,
                    marker,
                    bold,
                    italic,
                    link_url.as_deref(),
                    self.colors.last().copied(),
                );
                break;
            };
            let candidate = &marker[..=end];
            match parse_color_marker(candidate) {
                Some(color) => {
                    self.flush(out);
                    self.colors.push(color);
                }
                None => append_text(
                    &mut self.pending,
                    candidate,
                    bold,
                    italic,
                    link_url.as_deref(),
                    self.colors.last().copied(),
                ),
            }
            rest = &marker[end + 1..];
        }
    }

    /// Append a run when the caller has no block output to flush into. Tests
    /// that drive the parser directly use this; every parse path uses
    /// [`Self::push_text_to`] so a colour marker can split the run.
    #[cfg(test)]
    fn push_text(&mut self, text: &str) {
        self.push_text_to(text, &mut Vec::new());
    }

    fn flush(&mut self, out: &mut Vec<ChatFragment>) {
        // Whitespace that leads a block is dropped: it would indent the first
        // line. Whitespace anywhere else is the space between two runs of
        // different style, and dropping it draws those runs joined together.
        //
        // A block starts at the head of the list and after the line break that
        // separates it from the block above, so a run that opens the second
        // block is as much of an indent as one that opens the first.
        let leads_block = out.is_empty()
            || matches!(
                out.last().map(|fragment| &fragment.kind),
                Some(ChatFragmentKind::LineBreak)
            );
        if let Some(fragment) = take_pending(&mut self.pending, leads_block) {
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
    color: Option<ColorU>,
}

fn append_text(
    pending: &mut PendingText,
    text: &str,
    bold_count: usize,
    italic_count: usize,
    link_url: Option<&str>,
    color: Option<ColorU>,
) {
    // The run's style is the one in force where the run starts. A run that
    // starts in whitespace therefore stays plain, which is what the space
    // between two styled words is.
    if pending.text.is_empty() {
        pending.bold = bold_count > 0;
        pending.italic = italic_count > 0;
        pending.link_url = link_url.map(|s| s.to_string());
        pending.color = color;
    }
    pending.text.push_str(text);
}

/// The colour a `{color:…}` marker names: `#rrggbb` first, then the small
/// named set every client shares. `None` leaves the marker as literal text.
fn parse_color_marker(marker: &str) -> Option<ColorU> {
    let value = marker.strip_prefix("{color:")?.strip_suffix('}')?.trim();
    ColorU::from_hex(value).or_else(|| {
        let rgb = match value.to_ascii_lowercase().as_str() {
            "red" => 0xef4444,
            "orange" => 0xff9e64,
            "yellow" => 0xf59e0b,
            "green" => 0x10b981,
            "blue" => 0x19aad8,
            "purple" => 0xa78bfa,
            "cyan" => 0x22d3ee,
            "gray" | "grey" => 0x8e8e8e,
            "white" => 0xf1f1ef,
            "black" => 0x0e0e0e,
            _ => return None,
        };
        Some(ColorU::hex(rgb))
    })
}

/// The buffered run as a fragment, or `None` when there is nothing to draw.
///
/// The text is taken verbatim: the space between two runs of different style
/// belongs to one of them, and trimming it here draws those runs joined.
/// `leading` says the run opens a block, where whitespace is only an indent.
fn take_pending(pending: &mut PendingText, leading: bool) -> Option<ChatFragment> {
    let text = std::mem::take(&mut pending.text);
    let color = pending.color.take();
    let fragment = if text.is_empty() || (leading && text.trim().is_empty()) {
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
    fragment.map(|fragment| match color {
        Some(color) => fragment.with_color(color),
        None => fragment,
    })
}

fn merge_adjacent_text(fragments: &mut Vec<ChatFragment>) {
    let mut merged: Vec<ChatFragment> = Vec::with_capacity(fragments.len());
    for fragment in fragments.drain(..) {
        if let Some(last) = merged.last_mut() {
            // Two runs with different colours are not adjacent text of the same
            // style: merging them would paint the first run in the second's
            // colour.
            if last.color == fragment.color {
                if let (ChatFragmentKind::Text(a), ChatFragmentKind::Text(b)) =
                    (&last.kind, &fragment.kind)
                {
                    let separator = if a.ends_with(char::is_whitespace)
                        || b.starts_with(char::is_whitespace)
                    {
                        ""
                    } else {
                        " "
                    };
                    let combined = format!("{}{}{}", a, separator, b);
                    last.kind = ChatFragmentKind::Text(combined);
                    continue;
                }
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

    /// Every space of a paragraph is drawn. A run of text around a styled run is
    /// what carries the space between the two words, and trimming it away draws
    /// them joined: "foobarbaz".
    #[test]
    fn keeps_the_spaces_around_a_styled_run() {
        assert_eq!(
            parse_markdown("foo **bar** baz"),
            vec![
                ChatFragment::text("foo "),
                ChatFragment::bold("bar"),
                ChatFragment::text(" baz"),
            ]
        );
        assert_eq!(
            parse_markdown("**a** **b**"),
            vec![
                ChatFragment::bold("a"),
                ChatFragment::text(" "),
                ChatFragment::bold("b"),
            ]
        );
        assert_eq!(
            parse_markdown("a *b* c"),
            vec![
                ChatFragment::text("a "),
                ChatFragment::italic("b"),
                ChatFragment::text(" c"),
            ]
        );
        assert_eq!(
            parse_markdown("**a**  b"),
            vec![
                ChatFragment::bold("a"),
                ChatFragment::text("  b"),
            ]
        );
        assert_eq!(
            parse_markdown("one `code` two"),
            vec![
                ChatFragment::text("one "),
                ChatFragment::code("code"),
                ChatFragment::text(" two"),
            ]
        );
    }

    /// The same space is what separates the words of a heading, which is built
    /// by concatenating the runs it was parsed into.
    #[test]
    fn a_heading_keeps_the_spaces_between_its_runs() {
        assert_eq!(
            parse_markdown("# head **bold** tail"),
            vec![ChatFragment::heading(1, "head bold tail")]
        );
    }

    /// A run of several spaces is kept as it was written.
    #[test]
    fn keeps_a_run_of_spaces() {
        assert_eq!(
            parse_markdown("a  b   c"),
            vec![ChatFragment::text("a  b   c")]
        );
    }

    /// The space at the head or the tail of a paragraph is not content: it would
    /// indent the first line or trail off the last one.
    #[test]
    fn drops_the_whitespace_around_a_paragraph() {
        assert_eq!(parse_markdown("  hello  "), vec![ChatFragment::text("hello")]);
        assert_eq!(
            parse_markdown("one\n\ntwo"),
            vec![
                ChatFragment::text("one"),
                ChatFragment::line_break(),
                ChatFragment::text("two"),
            ]
        );
    }

    /// The whitespace rule on its own: a run that opens a block is dropped, a
    /// run inside a block is the space between two runs and is kept.
    #[test]
    fn a_run_that_opens_a_block_is_dropped_and_one_inside_it_is_kept() {
        let mut parser = MarkdownParser::new(Vec::new());
        parser.push_text(" ");
        let mut opening = Vec::new();
        parser.flush(&mut opening);
        assert!(
            opening.is_empty(),
            "a run that opens the message is not an indent, got {opening:?}"
        );

        parser.push_text(" ");
        let mut after_break = vec![ChatFragment::line_break()];
        parser.flush(&mut after_break);
        assert_eq!(
            after_break,
            vec![ChatFragment::line_break()],
            "a run that opens a later block is not an indent either"
        );

        parser.push_text(" ");
        let mut inside = vec![ChatFragment::text("one")];
        parser.flush(&mut inside);
        assert_eq!(
            inside,
            vec![ChatFragment::text("one"), ChatFragment::text(" ")],
            "a run inside a block is the space before the run that follows it"
        );
    }

    /// No block of a message opens with whitespace: an indent at the head of the
    /// first line is not something the source asked for, and the runs that do
    /// carry the spaces between words keep them.
    #[test]
    fn no_block_of_a_message_opens_with_whitespace() {
        for input in [
            "one\n\n  **two** three",
            "# head\n\n **two** three",
            "- item\n\n **two** three",
            "> quote\n\n **two** three",
            "one\n\n *two* three",
            "one\n\n\n\n  **two** three",
        ] {
            let fragments = parse_markdown(input);
            let opens_with_whitespace = fragments.iter().enumerate().any(|(index, fragment)| {
                let opens_a_block = index == 0
                    || matches!(
                        fragments[index - 1].kind,
                        ChatFragmentKind::LineBreak
                    );
                opens_a_block
                    && match &fragment.kind {
                        ChatFragmentKind::Text(text) => text.starts_with(char::is_whitespace),
                        _ => false,
                    }
            });
            assert!(
                !opens_with_whitespace,
                "{input:?} opens a block with whitespace: {fragments:?}"
            );
            // The words of the later block are still separated from each other.
            let drawn: String = fragments
                .iter()
                .filter_map(|fragment| match &fragment.kind {
                    ChatFragmentKind::Text(text)
                    | ChatFragmentKind::Bold(text)
                    | ChatFragmentKind::Italic(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                drawn.contains("two three"),
                "{input:?} keeps the space inside the block, got {drawn:?}"
            );
        }
    }

    #[test]
    fn parses_bold_and_italic() {
        let fragments = parse_markdown("**bold** _italic_");
        // The space between the two runs is kept: without it the paragraph
        // reads "bolditalic".
        assert_eq!(
            fragments,
            vec![
                ChatFragment::bold("bold"),
                ChatFragment::text(" "),
                ChatFragment::italic("italic"),
            ]
        );
    }

    #[test]
    fn parses_a_coloured_run() {
        assert_eq!(
            parse_markdown("{color:#ef4444}hot{/color}"),
            vec![ChatFragment::text("hot").with_color(ColorU::hex(0xef4444))]
        );
    }

    #[test]
    fn a_colour_wraps_a_bold_run() {
        assert_eq!(
            parse_markdown("{color:red}**hot**{/color}"),
            vec![ChatFragment::bold("hot").with_color(ColorU::hex(0xef4444))]
        );
    }

    #[test]
    fn a_named_colour_resolves() {
        assert_eq!(
            parse_markdown("{color:green}ok{/color}"),
            vec![ChatFragment::text("ok").with_color(ColorU::hex(0x10b981))]
        );
    }

    #[test]
    fn nested_colours_use_the_inner_one_then_resume() {
        assert_eq!(
            parse_markdown("{color:red}out {color:blue}in{/color} out{/color}"),
            vec![
                ChatFragment::text("out ").with_color(ColorU::hex(0xef4444)),
                ChatFragment::text("in").with_color(ColorU::hex(0x19aad8)),
                ChatFragment::text(" out").with_color(ColorU::hex(0xef4444)),
            ]
        );
    }

    #[test]
    fn an_unknown_colour_stays_literal() {
        assert_eq!(
            parse_markdown("{color:chartreuse}hot{/color}"),
            vec![ChatFragment::text("{color:chartreuse}hot{/color}")]
        );
    }

    #[test]
    fn an_unterminated_colour_marker_stays_literal() {
        assert_eq!(
            parse_markdown("a {color:#ff0000"),
            vec![ChatFragment::text("a {color:#ff0000")]
        );
    }

    #[test]
    fn a_close_without_an_open_stays_literal() {
        assert_eq!(
            parse_markdown("a {/color} b"),
            vec![ChatFragment::text("a {/color} b")]
        );
    }

    #[test]
    fn inline_code_keeps_the_marker_literal() {
        assert_eq!(
            parse_markdown("`{color:red}x{/color}`"),
            vec![ChatFragment::code("{color:red}x{/color}")]
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
                ChatFragment::text("see "),
                ChatFragment::link("Goble", "https://goble.dev"),
                ChatFragment::text(" for details"),
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
                ChatFragment::link("plain ", "https://goble.dev"),
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
            vec![ChatFragment::text("run "), ChatFragment::code("cargo build"),]
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
                        ChatFragment::text("plain "),
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



