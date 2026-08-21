use crate::elements::{ChatFragment, ChatFragmentKind};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Parse a Markdown string into a sequence of chat fragments.
///
/// Supported constructs:
/// - paragraphs and inline text
/// - bold (`**text**`), italic (`_text_`), bold+italic
/// - inline code and fenced code blocks (language is preserved)
/// - links `[label](url)`, including links containing bold/italic labels
/// - unordered/ordered lists (flattened to one `List` fragment)
/// - blockquotes
/// - headings (H1–H6)
/// - line breaks
pub fn parse_markdown(input: &str) -> Vec<ChatFragment> {
    let parser = Parser::new(input);
    let mut fragments = Vec::new();

    // Active inline style counters.
    let mut bold: usize = 0;
    let mut italic: usize = 0;
    let mut link_url: Option<String> = None;
    let mut pending = PendingText::default();

    // Structural containers.
    let mut list_items: Vec<String> = Vec::new();
    let mut list_ordered = false;
    let mut in_item = false;
    let mut item_text = String::new();

    let mut in_block_quote = false;
    let mut block_quote_text = String::new();

    let mut in_code_block = false;
    let mut code_block_text = String::new();
    let mut code_block_lang: Option<String> = None;

    let mut in_heading = false;
    let mut heading_level: u8 = 0;
    let mut heading_text = String::new();

    for event in parser {
        match event {
            Event::Start(tag) => {
                flush_pending(&mut pending, &mut fragments);
                match tag {
                    Tag::Strong => bold += 1,
                    Tag::Emphasis => italic += 1,
                    Tag::Link { dest_url, .. } => {
                        link_url = Some(dest_url.to_string());
                    }
                    Tag::List(order) => {
                        list_ordered = order.is_some();
                        list_items.clear();
                    }
                    Tag::Item => {
                        in_item = true;
                        item_text.clear();
                    }
                    Tag::BlockQuote(_) => {
                        in_block_quote = true;
                        block_quote_text.clear();
                    }
                    Tag::CodeBlock(lang) => {
                        in_code_block = true;
                        code_block_text.clear();
                        code_block_lang = match lang {
                            pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                                Some(lang.to_string())
                            }
                            pulldown_cmark::CodeBlockKind::Indented => None,
                        };
                    }
                    Tag::Heading { level, .. } => {
                        in_heading = true;
                        heading_level = level as u8;
                        heading_text.clear();
                    }
                    _ => {}
                }
            }
            Event::End(tag_end) => {
                flush_pending(&mut pending, &mut fragments);
                match tag_end {
                    TagEnd::Strong => bold = bold.saturating_sub(1),
                    TagEnd::Emphasis => italic = italic.saturating_sub(1),
                    TagEnd::Link => link_url = None,
                    TagEnd::List(_) => {
                        let items = std::mem::take(&mut list_items);
                        if !items.is_empty() {
                            fragments.push(ChatFragment::list(items, list_ordered));
                        }
                    }
                    TagEnd::Item => {
                        in_item = false;
                        let text = std::mem::take(&mut item_text).trim().to_string();
                        if !text.is_empty() {
                            list_items.push(text);
                        }
                    }
                    TagEnd::BlockQuote(_) => {
                        in_block_quote = false;
                        let text = std::mem::take(&mut block_quote_text).trim().to_string();
                        if !text.is_empty() {
                            fragments.push(ChatFragment::block_quote(text));
                        }
                    }
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        let code = std::mem::take(&mut code_block_text).trim().to_string();
                        let lang = code_block_lang.take();
                        if !code.is_empty() {
                            fragments.push(ChatFragment::code_block(lang, code));
                        }
                    }
                    TagEnd::Heading(_) => {
                        in_heading = false;
                        let text = std::mem::take(&mut heading_text).trim().to_string();
                        if !text.is_empty() {
                            fragments.push(ChatFragment::heading(heading_level, text));
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                if in_code_block {
                    code_block_text.push_str(&text);
                    code_block_text.push('\n');
                } else if in_block_quote {
                    block_quote_text.push_str(&text);
                } else if in_item {
                    item_text.push_str(&text);
                } else if in_heading {
                    heading_text.push_str(&text);
                } else {
                    append_text(&mut pending, &text, bold, italic, link_url.as_deref());
                }
            }
            Event::Code(code) => {
                if in_item {
                    item_text.push_str(&code);
                } else if in_block_quote {
                    block_quote_text.push_str(&code);
                } else {
                    flush_pending(&mut pending, &mut fragments);
                    fragments.push(ChatFragment::code(code.into_string()));
                }
            }
            Event::Html(html) => {
                if in_item {
                    item_text.push_str(&html);
                } else if in_block_quote {
                    block_quote_text.push_str(&html);
                } else if in_heading {
                    heading_text.push_str(&html);
                } else {
                    append_text(&mut pending, &html, bold, italic, link_url.as_deref());
                }
            }
            Event::SoftBreak => {
                if in_code_block {
                    code_block_text.push('\n');
                } else if in_block_quote {
                    block_quote_text.push(' ');
                } else if in_item {
                    item_text.push(' ');
                } else if in_heading {
                    heading_text.push(' ');
                } else {
                    append_text(&mut pending, " ", bold, italic, link_url.as_deref());
                }
            }
            Event::HardBreak => {
                if in_code_block {
                    code_block_text.push('\n');
                } else if in_block_quote {
                    block_quote_text.push('\n');
                } else if in_item {
                    item_text.push(' ');
                } else if in_heading {
                    heading_text.push(' ');
                } else {
                    flush_pending(&mut pending, &mut fragments);
                    fragments.push(ChatFragment::line_break());
                }
            }
            _ => {}
        }
    }

    flush_pending(&mut pending, &mut fragments);
    merge_adjacent_text(&mut fragments);
    fragments
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

fn flush_pending(pending: &mut PendingText, fragments: &mut Vec<ChatFragment>) {
    let text = std::mem::take(&mut pending.text).trim().to_string();
    if text.is_empty() {
        return;
    }
    let fragment = if let Some(url) = pending.link_url.as_ref() {
        ChatFragment::link(text, url.clone())
    } else if pending.bold && pending.italic {
        ChatFragment::bold_italic(text)
    } else if pending.bold {
        ChatFragment::bold(text)
    } else if pending.italic {
        ChatFragment::italic(text)
    } else {
        ChatFragment::text(text)
    };
    fragments.push(fragment);
}

fn merge_adjacent_text(fragments: &mut Vec<ChatFragment>) {
    let mut merged: Vec<ChatFragment> = Vec::with_capacity(fragments.len());
    for fragment in fragments.drain(..) {
        if let Some(last) = merged.last_mut() {
            if let (ChatFragmentKind::Text(a), ChatFragmentKind::Text(b)) = (&last.kind, &fragment.kind)
            {
                let separator = if a.ends_with(char::is_whitespace) || b.starts_with(char::is_whitespace)
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
        merged.push(fragment);
    }
    *fragments = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text() {
        let fragments = parse_markdown("hello world");
        assert_eq!(fragments, vec![ChatFragment::text("hello world")]);
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
    fn parses_inline_code() {
        let fragments = parse_markdown("run `cargo build`");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::text("run"),
                ChatFragment::code("cargo build"),
            ]
        );
    }

    #[test]
    fn parses_code_block() {
        let fragments = parse_markdown("```rust\nfn main() {}\n```");
        assert_eq!(
            fragments,
            vec![ChatFragment::code_block(Some("rust".to_string()), "fn main() {}")]
        );
    }

    #[test]
    fn parses_unordered_list() {
        let fragments = parse_markdown("- one\n- two");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(vec!["one".into(), "two".into()], false)]
        );
    }

    #[test]
    fn parses_ordered_list() {
        let fragments = parse_markdown("1. first\n2. second");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(vec!["first".into(), "second".into()], true)]
        );
    }

    #[test]
    fn parses_list_with_styled_item() {
        let fragments = parse_markdown("- **one**\n- plain **bold**");
        assert_eq!(
            fragments,
            vec![ChatFragment::list(
                vec!["one".into(), "plain bold".into()],
                false
            )]
        );
    }

    #[test]
    fn parses_blockquote() {
        let fragments = parse_markdown("> quote");
        assert_eq!(fragments, vec![ChatFragment::block_quote("quote")]);
    }

    #[test]
    fn parses_headings() {
        let fragments = parse_markdown("# H1\n## H2\n### H3");
        assert_eq!(
            fragments,
            vec![
                ChatFragment::heading(1, "H1"),
                ChatFragment::heading(2, "H2"),
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
