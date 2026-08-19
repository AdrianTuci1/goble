use crate::elements::{ChatFragment, ChatFragmentKind};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Parse a Markdown string into a sequence of chat fragments.
///
/// Supported constructs:
/// - paragraphs and inline text
/// - bold (`**text**`), italic (`_text_`), bold+italic
/// - inline code and fenced code blocks
/// - links `[label](url)`
/// - unordered/ordered lists (flattened to one `List` fragment)
/// - blockquotes
/// - line breaks
pub fn parse_markdown(input: &str) -> Vec<ChatFragment> {
    let parser = Parser::new(input);
    let mut fragments = Vec::new();
    let mut style_stack: Vec<StyleFrame> = Vec::new();
    let mut list_items: Vec<String> = Vec::new();
    let mut block_quote_text = String::new();
    let mut code_block_text = String::new();
    let mut in_code_block = false;
    let mut in_block_quote = false;
    let mut pending_text = String::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => push_style(&mut style_stack, Style::Bold, None),
                Tag::Emphasis => push_style(&mut style_stack, Style::Italic, None),
                Tag::Link { dest_url, .. } => push_style(&mut style_stack, Style::Link, Some(dest_url.to_string())),
                Tag::List(_) => {
                    flush_text(&mut pending_text, &style_stack, &mut fragments);
                    list_items.clear();
                }
                Tag::Item => {
                    flush_text(&mut pending_text, &style_stack, &mut fragments);
                }
                Tag::BlockQuote(_) => {
                    flush_text(&mut pending_text, &style_stack, &mut fragments);
                    in_block_quote = true;
                    block_quote_text.clear();
                }
                Tag::CodeBlock(_) => {
                    flush_text(&mut pending_text, &style_stack, &mut fragments);
                    in_code_block = true;
                    code_block_text.clear();
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => pop_style(&mut style_stack, Style::Bold, &mut fragments),
                TagEnd::Emphasis => pop_style(&mut style_stack, Style::Italic, &mut fragments),
                TagEnd::Link => pop_link(&mut style_stack, &mut fragments),
                TagEnd::List(ordered) => {
                    flush_text(&mut pending_text, &style_stack, &mut fragments);
                    let items = std::mem::take(&mut list_items);
                    if !items.is_empty() {
                        fragments.push(ChatFragment::list(items, ordered));
                    }
                }
                TagEnd::Item => {
                    let item_text = take_pending_text(&mut pending_text, &style_stack);
                    if !item_text.is_empty() {
                        list_items.push(item_text);
                    }
                }
                TagEnd::BlockQuote(_) => {
                    let text = std::mem::take(&mut block_quote_text);
                    in_block_quote = false;
                    if !text.is_empty() {
                        fragments.push(ChatFragment::block_quote(text.trim().to_string()));
                    }
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    let code = std::mem::take(&mut code_block_text).trim().to_string();
                    if !code.is_empty() {
                        fragments.push(ChatFragment::code(code));
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    code_block_text.push_str(&text);
                    code_block_text.push('\n');
                } else if in_block_quote {
                    block_quote_text.push_str(&text);
                } else if let Some(frame) = style_stack.last_mut() {
                    frame.buffer.push_str(&text);
                } else {
                    pending_text.push_str(&text);
                }
            }
            Event::Code(code) => {
                flush_text(&mut pending_text, &style_stack, &mut fragments);
                fragments.push(ChatFragment::code(code.into_string()));
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block {
                    code_block_text.push('\n');
                } else if in_block_quote {
                    block_quote_text.push('\n');
                } else if let Some(frame) = style_stack.last_mut() {
                    frame.buffer.push(' ');
                } else {
                    flush_text(&mut pending_text, &style_stack, &mut fragments);
                    fragments.push(ChatFragment::line_break());
                }
            }
            Event::Html(html) => {
                // For safety, render raw HTML as plain text.
                if let Some(frame) = style_stack.last_mut() {
                    frame.buffer.push_str(&html);
                } else {
                    pending_text.push_str(&html);
                }
            }
            _ => {}
        }
    }

    flush_text(&mut pending_text, &style_stack, &mut fragments);
    merge_adjacent_text(&mut fragments);
    fragments
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Bold,
    Italic,
    BoldItalic,
    Link,
}

#[derive(Debug)]
struct StyleFrame {
    style: Style,
    buffer: String,
    link_url: Option<String>,
}

fn push_style(stack: &mut Vec<StyleFrame>, style: Style, link_url: Option<String>) {
    // Combine inline styles rather than nesting frames.
    if let Some(top) = stack.last_mut() {
        top.style = combine_styles(top.style, style);
    } else {
        stack.push(StyleFrame {
            style,
            buffer: String::new(),
            link_url,
        });
    }
}

fn combine_styles(existing: Style, new: Style) -> Style {
    match (existing, new) {
        (Style::Bold, Style::Italic) | (Style::Italic, Style::Bold) => Style::BoldItalic,
        (Style::Bold, Style::BoldItalic) | (Style::Italic, Style::BoldItalic) => Style::BoldItalic,
        (Style::BoldItalic, _) => Style::BoldItalic,
        _ => new,
    }
}

fn pop_style(stack: &mut Vec<StyleFrame>, expected: Style, fragments: &mut Vec<ChatFragment>) {
    if let Some(frame) = stack.pop() {
        let text = frame.buffer.trim().to_string();
        if !text.is_empty() {
            let fragment = match frame.style {
                Style::Bold => ChatFragment::bold(text),
                Style::Italic => ChatFragment::italic(text),
                Style::BoldItalic => ChatFragment::bold_italic(text),
                Style::Link => ChatFragment::text(text),
            };
            fragments.push(fragment);
        }
    }
    let _ = expected;
}

fn pop_link(stack: &mut Vec<StyleFrame>, fragments: &mut Vec<ChatFragment>) {
    if let Some(frame) = stack.pop() {
        let text = frame.buffer.trim().to_string();
        if !text.is_empty() {
            let url = frame.link_url.unwrap_or_else(|| text.clone());
            fragments.push(ChatFragment::link(text, url));
        }
    }
}

fn take_pending_text(pending: &mut String, stack: &[StyleFrame]) -> String {
    if let Some(frame) = stack.last() {
        frame.buffer.clone()
    } else {
        std::mem::take(pending).trim().to_string()
    }
}

fn flush_text(
    pending: &mut String,
    stack: &[StyleFrame],
    fragments: &mut Vec<ChatFragment>,
) {
    if let Some(frame) = stack.last() {
        let text = frame.buffer.trim();
        if !text.is_empty() {
            let fragment = match frame.style {
                Style::Bold => ChatFragment::bold(text.to_string()),
                Style::Italic => ChatFragment::italic(text.to_string()),
                Style::BoldItalic => ChatFragment::bold_italic(text.to_string()),
                Style::Link => ChatFragment::text(text.to_string()),
            };
            fragments.push(fragment);
        }
    } else {
        let text = std::mem::take(pending).trim().to_string();
        if !text.is_empty() {
            fragments.push(ChatFragment::text(text));
        }
    }
}

fn merge_adjacent_text(fragments: &mut Vec<ChatFragment>) {
    let mut merged: Vec<ChatFragment> = Vec::with_capacity(fragments.len());
    for fragment in fragments.drain(..) {
        if let Some(last) = merged.last_mut() {
            match (&last.kind, &fragment.kind) {
                (
                    ChatFragmentKind::Text(a),
                    ChatFragmentKind::Text(b),
                ) => {
                    let combined = format!("{} {}", a, b);
                    last.kind = ChatFragmentKind::Text(combined);
                    continue;
                }
                _ => {}
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
            vec![
                ChatFragment::bold("bold"),
                ChatFragment::italic("italic"),
            ]
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
        assert_eq!(fragments, vec![ChatFragment::code("fn main() {}")]);
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
    fn parses_blockquote() {
        let fragments = parse_markdown("> quote");
        assert_eq!(fragments, vec![ChatFragment::block_quote("quote")]);
    }
}
