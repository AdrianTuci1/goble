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
fn a_coloured_run_is_painted_in_its_colour() {
    assert_eq!(
        run_color("{color:#ef4444}x{/color}"),
        ColorU::hex(0xef4444),
        "the marker's colour must reach the painted glyphs"
    );
}

/// Inline code is one accent run. A list of paths must not become a wall of
/// raised chips, which is what a per-run background box reads as.
#[test]
fn inline_code_is_one_accent_run_without_a_box() {
    use crate::theme::FontFamily;

    let app = AppContext::default();
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "Directories: `crates/`, `app/`, `docs/`",
    );
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    let commands = paint_bubble(&mut bubble, &app);

    for word in ["crates/", "app/", "docs/"] {
        let run = text_runs(&commands)
            .into_iter()
            .find(|(text, ..)| text == word)
            .unwrap_or_else(|| panic!("{word:?} is drawn, got {commands:?}"));
        assert_eq!(
            run.1,
            app.theme.color(ColorToken::Accent),
            "{word:?} takes the accent colour"
        );
        assert_eq!(run.2, FontFamily::Mono, "{word:?} stays code-shaped");
        assert_eq!(run.3, 14.0, "{word:?} sits at the message's size");
    }
    let raised = app.theme.color(ColorToken::SurfaceRaised);
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == raised
        )),
        "inline code draws no raised box, got {commands:?}"
    );
}

#[test]
fn reasoning_row_is_recessed_and_collapsed_until_expanded() {
    use crate::color::ColorU;

    let app = AppContext::default();
    let expanded = Rc::new(RefCell::new(HashMap::new()));
    let key = "c1:0";

    let collapsed = reasoning_commands(&expanded, key);
    let texts: Vec<(String, ColorU, FontWeight)> = collapsed
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text,
                color,
                font_weight,
                ..
            } => Some((text.clone(), *color, *font_weight)),
            _ => None,
        })
        .collect();

    let header = texts
        .iter()
        .find(|(text, _, _)| text.contains("Thought"))
        .unwrap_or_else(|| panic!("the reasoning header must be drawn, got {texts:?}"));
    assert_eq!(
        header.1,
        app.theme.color(ColorToken::Muted),
        "the reasoning row must be recessed (muted), got {header:?}"
    );
    assert_eq!(
        header.2,
        FontWeight::Bold,
        "grok-build's Thought label is bold, got {header:?}"
    );
    assert_eq!(header.0, "Thought", "the finished label is exactly grok-build's");
    assert!(
        texts.iter().any(|(text, _, _)| text == "◆ "),
        "a thinking row wears the tool diamond while its header is visible, got {texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|(text, _, _)| text.contains("weighing the options")),
        "the body must be hidden while collapsed, got {texts:?}"
    );

    // The app-owned flag is what opens the row.
    expanded.borrow_mut().insert(key.to_string(), true);
    let open_commands = reasoning_commands(&expanded, key);
    let open: Vec<String> = open_commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        open.iter()
            .any(|text| text.contains("weighing the options")),
        "an expanded row draws its body, got {open:?}"
    );
    assert!(
        open_commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { rect, .. } if rect.width() == QUOTE_RAIL_WIDTH
        )),
        "an expanded row draws the rail beside its body, got {open_commands:?}"
    );
}

#[test]
fn a_streaming_reasoning_row_is_open_until_it_finishes() {
    let expanded = Rc::new(RefCell::new(HashMap::new()));
    let key = "c1:0";

    let live: Vec<String> = reasoning_commands_in(&expanded, key, false)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        live.iter().any(|text| text == "Thinking…"),
        "the live row says it is still thinking, got {live:?}"
    );
    assert!(
        live.iter()
            .any(|text| text.contains("weighing the options")),
        "the live thinking body is drawn while it streams, got {live:?}"
    );
    assert!(
        expanded.borrow().is_empty(),
        "streaming needs no manual fold state"
    );

    let finished: Vec<String> = reasoning_commands_in(&expanded, key, true)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        finished.iter().any(|text| text == "Thought"),
        "a finished step is grok-build's one-line Thought row, got {finished:?}"
    );
    assert!(
        !finished
            .iter()
            .any(|text| text.contains("weighing the options")),
        "the finished body folds away, got {finished:?}"
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

/// The agent's coloured output is chat text: the escapes colour the glyphs and
/// the block draws no raised panel behind them.
#[test]
fn an_ansi_fence_paints_coloured_prose_without_a_panel() {
    use crate::theme::FontFamily;

    let app = AppContext::default();
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "```ansi\n\u{1b}[31mred\u{1b}[0m and \u{1b}[1;32mbold green\u{1b}[0m\n```",
    );
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

    let runs = text_runs(&commands);
    let run = |needle: &str| {
        runs.iter()
            .find(|(text, ..)| text == needle)
            .unwrap_or_else(|| panic!("{needle:?} is drawn, got {runs:?}"))
    };
    let red = run("red");
    assert_eq!(red.1, ColorU::new(205, 0, 0, 255), "the named foreground colours the run");
    assert_eq!(red.2, FontFamily::System, "coloured output is prose, not code");
    let green = run("bold green");
    assert_eq!(green.1, ColorU::new(0, 205, 0, 255));
    assert_eq!(green.3, 14.0, "coloured output sits at the message's size");

    let panel = app.theme.color(ColorToken::SurfaceRaised);
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == panel
        )),
        "coloured output draws no code panel, got {commands:?}"
    );
}

/// The model may spell the escape as text. The transcript reads it the same way
/// rather than showing a backslash sequence.
#[test]
fn a_textual_escape_in_an_ansi_fence_still_colours_the_run() {
    let app = AppContext::default();
    let message =
        ChatMessage::from_markdown(ChatRole::Assistant, "```\n\\033[35mviolet\\033[0m\n```");
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

    let violet = text_runs(&commands)
        .into_iter()
        .find(|(text, ..)| text == "violet")
        .unwrap_or_else(|| panic!("the coloured run is drawn, got {commands:?}"));
    assert_eq!(violet.1, ColorU::new(205, 0, 205, 255));
}

/// The block a model actually wrote when asked for colour: a plain fence of
/// textual SGR escapes. It is painted as colour now, not shown as backslashes.
#[test]
fn the_colour_a_model_wrote_in_a_plain_fence_is_painted() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "```\n\\033[31mROȘU\\033[0m  \\033[32mVERDE\\033[0m  \\033[34mALBASTRU\\033[0m\n\
         \\033[33mGALBEN\\033[0m  \\033[35mMAGENTA\\033[0m  \\033[36mCYAN\\033[0m\n```",
    );
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

    let runs = text_runs(&commands);
    let colors: Vec<ColorU> = ["ROȘU", "VERDE", "ALBASTRU", "GALBEN", "MAGENTA", "CYAN"]
        .iter()
        .map(|word| {
            runs.iter()
                .find(|(text, ..)| text == word)
                .map(|(_, color, ..)| *color)
                .unwrap_or_else(|| panic!("{word:?} is drawn, got {runs:?}"))
        })
        .collect();
    assert_eq!(
        colors,
        vec![
            ColorU::new(205, 0, 0, 255),
            ColorU::new(0, 205, 0, 255),
            ColorU::new(0, 0, 238, 255),
            ColorU::new(205, 205, 0, 255),
            ColorU::new(205, 0, 205, 255),
            ColorU::new(0, 205, 205, 255),
        ],
        "each named foreground reaches its word"
    );
    assert!(
        !runs.iter().any(|(text, ..)| text.contains("\\033")),
        "the escapes are never drawn as text, got {runs:?}"
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

/// A markdown list draws grok-build's muted bullet before every item, and an
/// ordered list keeps its source numbers. The marker is its own run so the
/// item body keeps the paragraph colour and a wrapped item hangs under its
/// first line.
#[test]
fn lists_draw_muted_bullets_and_ordered_numbers() {
    let app = AppContext::default();
    let message =
        ChatMessage::from_markdown(ChatRole::Assistant, "- one\n- two\n\n1. first\n2. second");
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();
    let texts = text_runs(&commands);

    let markers: Vec<(&str, ColorU)> = texts
        .iter()
        .filter_map(|(text, color, ..)| match text.as_str() {
            "• " | "1. " | "2. " => Some((text.as_str(), *color)),
            _ => None,
        })
        .collect();
    assert_eq!(
        markers,
        vec![
            ("• ", app.theme.color(ColorToken::Muted)),
            ("• ", app.theme.color(ColorToken::Muted)),
            ("1. ", app.theme.color(ColorToken::Muted)),
            ("2. ", app.theme.color(ColorToken::Muted)),
        ],
        "bullets and numbers are muted markers, got {texts:?}"
    );
    for item in ["one", "two", "first", "second"] {
        assert!(
            texts.iter().any(|(text, ..)| text == item),
            "the item body {item:?} is drawn, got {texts:?}"
        );
    }
}
