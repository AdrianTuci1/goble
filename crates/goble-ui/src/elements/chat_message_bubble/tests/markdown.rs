use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::SizeConstraint;
use crate::elements::chat_content::ChatMessage;
use crate::elements::chat_content::{ChatFragment, ChatRole};
use crate::elements::{AppContext, LayoutContext, PaintContext};
use crate::event::DispatchedEvent;
use crate::geometry::vec2f;
use crate::platform::text_atlas::FontWeight;
use crate::render::{RenderCommand, Renderer};
use crate::theme::ColorToken;
use super::*;
use super::super::{ChatMessageBubble, QUOTE_RAIL_WIDTH};

#[test]
fn italic_is_not_bold_and_bold_italic_is_both() {
    assert_eq!(
        run_style("_x_"),
        (FontWeight::Regular, true),
        "_x_ must render italic and not bold"
    );
    assert_eq!(
        run_style("***x***"),
        (FontWeight::Bold, true),
        "***x*** must render bold and italic"
    );
    assert_eq!(
        run_style("**x**"),
        (FontWeight::Bold, false),
        "**x** must stay bold-only"
    );
}

#[test]
fn reasoning_row_is_recessed_and_collapsed_until_expanded() {
    use crate::color::ColorU;

    let app = AppContext::default();
    let expanded = Rc::new(RefCell::new(HashMap::new()));
    let key = "c1:0";

    let collapsed = reasoning_commands(&expanded, key);
    let texts: Vec<(String, ColorU)> = collapsed
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();

    let header = texts
        .iter()
        .find(|(text, _)| text.contains("Thinking"))
        .unwrap_or_else(|| panic!("the reasoning header must be drawn, got {texts:?}"));
    assert_eq!(
        header.1,
        app.theme.color(ColorToken::Muted),
        "the reasoning row must be recessed (muted), got {header:?}"
    );
    assert!(
        texts.iter().any(|(text, _)| text == "▸ "),
        "a collapsed row shows the closed marker, got {texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|(text, _)| text.contains("weighing the options")),
        "the body must be hidden while collapsed, got {texts:?}"
    );

    // The app-owned flag is what opens the row.
    expanded.borrow_mut().insert(key.to_string(), true);
    let open: Vec<String> = reasoning_commands(&expanded, key)
        .into_iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert!(
        open.iter()
            .any(|text| text.contains("weighing the options")),
        "an expanded row draws its body, got {open:?}"
    );
    assert!(
        open.iter().any(|text| text == "▾ "),
        "an expanded row shows the open marker, got {open:?}"
    );
}

#[test]
fn reasoning_header_click_toggles_the_row() {
    let app = AppContext::default();
    let expanded = Rc::new(RefCell::new(HashMap::new()));
    let key = "c1:0";
    let mut bubble = ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::reasoning(key, "contemplating", "body", true)],
    )
    .with_reasoning_expanded(expanded.clone());
    bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    bubble.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut event_ctx = crate::elements::EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position: vec2f(20.0, 20.0),
        button: 0,
    };
    let up = DispatchedEvent::MouseUp {
        position: vec2f(20.0, 20.0),
        button: 0,
    };
    assert!(bubble.dispatch_event(&down, &mut event_ctx, &app));
    assert!(bubble.dispatch_event(&up, &mut event_ctx, &app));
    assert_eq!(
        expanded.borrow().get(key),
        Some(&true),
        "clicking the header must expand the row"
    );
}

#[test]
fn fenced_code_block_paints_in_the_mono_family() {
    use crate::theme::FontFamily;

    let app = AppContext::default();
    let message = ChatMessage::from_markdown(ChatRole::Assistant, "```rust\nlet x = 1;\n```");
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let runs: Vec<(String, FontFamily)> = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text, font_family, ..
            } => Some((text, font_family)),
            _ => None,
        })
        .collect();

    assert!(
        runs.iter()
            .any(|(text, family)| text == "rust" && *family == FontFamily::Mono),
        "the fence's language label must be drawn in mono, got {runs:?}"
    );
    assert!(
        runs.iter().all(|(_, family)| *family == FontFamily::Mono),
        "every fenced run must be mono, got {runs:?}"
    );
    // The body is highlighted now, so it arrives as several coloured runs;
    // the label is the one run that is not part of it.
    let body: String = runs
        .iter()
        .filter(|(text, _)| text != "rust")
        .map(|(text, _)| text.as_str())
        .collect();
    assert_eq!(
        body, "let x = 1;",
        "the fenced body must be drawn verbatim, got {runs:?}"
    );
}

/// H5: a fenced block is a full-width panel band, never a rounded box, and
/// the fence label and the highlighted body survive the change.
#[test]
fn fenced_code_block_is_a_band_not_a_rounded_box() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(ChatRole::Assistant, "```rust\nlet x = 1;\n```");
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

    let band = app.theme.color(ColorToken::SurfaceRaised);
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, corner_radius, .. }
                if *color == band && *corner_radius > 0.0
        )),
        "the fence must not be a rounded box, got {commands:?}"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, corner_radius, .. }
                if *color == band && *corner_radius == 0.0
        )),
        "the fence must sit on a flat band, got {commands:?}"
    );

    let texts = text_runs(&commands);
    assert!(
        texts.iter().any(|(text, ..)| text == "rust"),
        "the fence's language label must survive, got {texts:?}"
    );
    let body_colours: std::collections::HashSet<ColorU> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, color, .. } if text != "rust" => Some(*color),
            _ => None,
        })
        .collect();
    assert!(
        body_colours.len() > 1,
        "the fenced body must stay highlighted, got {body_colours:?}"
    );
}

/// H5: a blockquote is an indent plus a rail, not a rounded box.
#[test]
fn block_quote_is_an_indent_and_a_rail() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(ChatRole::Assistant, "> quoted line");
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

    let box_bg = app.theme.color(ColorToken::SurfaceRaised);
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, .. } if *color == box_bg
        )),
        "a quote must not paint a raised box, got {commands:?}"
    );
    let rail = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if rect.width() == QUOTE_RAIL_WIDTH => Some((*color, *corner_radius)),
            _ => None,
        })
        .expect("the quote must paint a rail");
    assert_eq!(rail.1, 0.0, "the rail is square");
    assert_eq!(
        rail.0,
        app.theme.color(ColorToken::Muted),
        "the rail is muted"
    );

    let texts = text_runs(&commands);
    assert!(
        texts.iter().any(|(text, ..)| text.contains("quoted line")),
        "the quoted text must be drawn, got {texts:?}"
    );
}
