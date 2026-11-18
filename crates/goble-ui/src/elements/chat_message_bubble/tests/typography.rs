//! The acceptance lock for the chat typography pass: DSH's size, line-height,
//! weight, ink and shape per surface, the user-row width invariant the owner
//! required, and the Settings linkage that keeps every one of those numbers
//! derived from the theme rather than baked in.
//!
//! These assertions read the paint commands and layout rects the elements
//! actually emit, so a later restyle that silently changes a size or a colour
//! fails here rather than only in a screenshot.

use super::*;
use crate::elements::chat_typography;
use crate::elements::chat_content::{ChatFragment, ChatRole};
use crate::theme::{Density, Theme, ThemePreset};

/// The first drawn run whose text starts with `prefix`, as
/// `(font_size, line_height, colour, weight, family)`.
fn runmatching(
    commands: &[RenderCommand],
    prefix: &str,
) -> (f32, f32, ColorU, FontWeight, FontFamily) {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                text,
                font_size,
                line_height,
                color,
                font_weight,
                font_family,
                ..
            } if text.starts_with(prefix) => {
                Some((*font_size, *line_height, *color, *font_weight, *font_family))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the run {prefix:?} is drawn, got {commands:?}"))
}

/// The `x` the first drawn run starting with `prefix` begins at.
fn text_x(commands: &[RenderCommand], prefix: &str) -> f32 {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text.starts_with(prefix) => {
                Some(origin.x)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the run {prefix:?} is drawn"))
}

fn approx(what: &str, got: f32, want: f32) {
    assert!(
        (got - want).abs() < 1e-4,
        "{what}: got {got}, want {want}"
    );
}

/// A bubble with one fragment renders its paint commands at `width`.
fn fragments_commands(
    app: &AppContext,
    role: ChatRole,
    fragments: Vec<ChatFragment>,
    width: f32,
) -> (Vec<RenderCommand>, Vector2F) {
    let mut element: Box<dyn Element> = Box::new(ChatMessageBubble::new(role, fragments));
    let commands = render_element(&mut element, vec2f(width, 800.0), app);
    let size = element.size().expect("the row is laid out");
    (commands, size)
}

/// §6.1: every surface the owner named draws DSH's type — the same size, line
/// height, weight, ink role and family (through the Settings seam) at defaults.
#[test]
fn every_chat_surface_draws_dsh_type() {
    let app = AppContext::default();
    let family = chat_typography::chat_font(&app);
    assert_eq!(family, FontFamily::System, "the default chat family");

    // Assistant prose: 14 / 24, regular, primary ink, the theme family.
    let (commands, _) = fragments_commands(
        &app,
        ChatRole::Assistant,
        vec![ChatFragment::text("Salut, monde.")],
        600.0,
    );
    let (size, line, color, weight, run_family) = runmatching(&commands, "Salut");
    approx("assistant prose size", size, 14.0);
    approx("assistant prose line height", line, chat_typography::BODY_LH);
    assert_eq!(color, app.theme.color(ColorToken::Text), "assistant ink");
    assert_eq!(weight, FontWeight::Regular);
    assert_eq!(run_family, family);

    // The user's own row: DSH's 14 / 22 bubble line, primary ink, the marker
    // still Goble's Accent `❯` on the first line.
    let (commands, size) = fragments_commands(
        &app,
        ChatRole::User,
        vec![ChatFragment::text("Salut, monde.")],
        600.0,
    );
    approx("user row width", size.x, 600.0);
    let (size, line, color, weight, run_family) = runmatching(&commands, "Salut");
    approx("user message size", size, 14.0);
    approx("user message line height", line, chat_typography::USER_LH);
    assert_eq!(color, app.theme.color(ColorToken::Text), "user ink");
    assert_eq!(weight, FontWeight::Regular);
    assert_eq!(run_family, family);
    let marker = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                name, origin, size, ..
            } if name == "prompt-arrow" => Some((*origin, *size)),
            _ => None,
        })
        .expect("the user's row keeps its prompt marker");
    let text_y = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text: drawn,
                ..
            } if drawn == "Salut, monde." => Some(origin.y),
            _ => None,
        })
        .expect("the message is drawn");
    assert!(
        (marker.0.y + marker.1 / 2.0 - (text_y + 14.0 * chat_typography::USER_LH / 2.0)).abs()
            <= 1.0,
        "the marker is centred on the 22 px user line, got {marker:?} for a line at {text_y}"
    );
    approx(
        "the marker sits on the band's 16 px inset",
        marker.0.x,
        chat_typography::BAND_PADDING_X,
    );

    // Reasoning: a 13 / 24 regular disclosure row over a 13 / 20 body, both in
    // the quiet ink and the prose family (no mono, no bold).
    let mut bubble = ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::reasoning("c1:0", "contemplating", "weighing it", true)],
    );
    let commands = paint_bubble(&mut bubble, &app);
    let (size, line, color, weight, run_family) = runmatching(&commands, "Thought");
    approx("reasoning title size", size, 13.0);
    approx("reasoning title line", line, chat_typography::ROW_LH);
    assert_eq!(color, app.theme.color(ColorToken::Muted), "reasoning ink");
    assert_eq!(weight, FontWeight::Regular, "the reasoning row is not bold");
    assert_eq!(run_family, family, "the reasoning row is prose, not mono");

    let expanded = Rc::new(RefCell::new(HashMap::new()));
    let mut open = ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::reasoning("c1:0", "done", "weighing it", true)],
    )
    .with_reasoning_expanded(expanded.clone());
    expanded.borrow_mut().insert("c1:0".to_string(), true);
    let commands = paint_bubble(&mut open, &app);
    let (size, line, _color, _weight, run_family) = runmatching(&commands, "weighing");
    approx("reasoning body size", size, 13.0);
    approx("reasoning body line", line, chat_typography::ROW_TIGHT_LH);
    assert_eq!(run_family, family);

    // A notice: one 13 / 24 quiet line.
    let (commands, _) = fragments_commands(
        &app,
        ChatRole::Assistant,
        vec![ChatFragment::notice("Context 82% full.")],
        600.0,
    );
    let (size, line, color, weight, run_family) = runmatching(&commands, "Context");
    approx("notice size", size, 13.0);
    approx("notice line", line, chat_typography::ROW_LH);
    assert_eq!(color, app.theme.color(ColorToken::Muted));
    assert_eq!(weight, FontWeight::Regular);
    assert_eq!(run_family, family);

    // A tool row: 13 / 24, regular, quiet ink, prose family, Goble's `◆` kept.
    // A tool no family folds (here `credentials`) stands on its own, so the row
    // wears the single-call diamond.
    let (commands, _) = paint_tool_calls(vec![call(
        "credentials",
        r#"{"agent_id":"researcher"}"#,
        ToolCallStatus::Finished,
        Some("none"),
    )]);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text == "◆"
        )),
        "the tool row keeps Goble's diamond, got {commands:?}"
    );
    let (size, line, color, weight, run_family) = runmatching(&commands, "credentials");
    approx("tool row size", size, 13.0);
    approx("tool row line", line, chat_typography::ROW_LH);
    assert_eq!(color, app.theme.color(ColorToken::Muted), "a resting row is quiet");
    assert_eq!(weight, FontWeight::Regular, "the verb is no longer bold");
    assert_eq!(run_family, family, "the tool row is prose, not mono");

    // A failed call's summary takes the error ink.
    let (commands, _) = paint_tool_calls(vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Error,
        Some("no such file"),
    )]);
    // A failed call's summary is DSH's error ink. A `read` folds into its
    // run's header, which states the failure count in that ink.
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, color, .. }
                if text.contains("failed")
                    && *color == app.theme.color(ColorToken::Error)
        )),
        "a failed call's summary is the error ink, got {commands:?}"
    );

    // A fenced block: the 11 / 19 mono body inside DSH's quiet code card.
    let (commands, _) = fragments_commands(
        &app,
        ChatRole::Assistant,
        vec![ChatFragment::code_block(Some("rust".into()), "let x = 1;")],
        600.0,
    );
    // The body is syntax-highlighted, so its runs carry the highlighter's
    // colours; the type and the mono family are what the card fixes.
    let (size, line, _color, _weight, run_family) = runmatching(&commands, "let");
    approx("code body size", size, chat_typography::CODE_SIZE);
    approx("code body line", line, chat_typography::CODE_LH);
    assert_eq!(run_family, FontFamily::Mono, "code stays the mono contract");
    let card_radius = chat_typography::card_radius(&app);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::StrokeRect { width, corner_radius, .. }
                if (*width - chat_typography::HAIRLINE).abs() < 1e-5
                    && (*corner_radius - card_radius).abs() < 0.01
        )),
        "the fence draws DSH's hairline card at radius {card_radius}"
    );
}

/// §6.2: the user and agent rows share the pane's width and left edge. The
/// prompt marker is the only permitted difference between their text origins;
/// neither row is right-aligned and neither is narrower than the other.
#[test]
fn user_and_agent_rows_share_the_width_and_left_edge() {
    let app = AppContext::default();
    let width = 600.0;

    let (reply, reply_size) = paint_row(ChatRole::Assistant, "Salut!", width);
    let (message, message_size) = paint_row(
        ChatRole::User,
        "Salut! This user message is deliberately longer than the agent's reply, so the two rows cannot agree by accident.",
        width,
    );

    approx("the agent's row spans the pane", reply_size.x, width);
    approx("the user's row spans the pane", message_size.x, width);

    // The user's band is the full pane edge to edge and square — no inset,
    // no right-aligned capped bubble.
    let band = message
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised) => Some((*rect, *corner_radius)),
            _ => None,
        })
        .expect("the user's row paints its band");
    approx("the user band's left edge", band.0.min_x(), 0.0);
    approx("the user band's right edge", band.0.max_x(), width);
    approx("the user band's radius", band.1, 0.0);
    assert!(
        !reply.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. }
                if *color == app.theme.color(ColorToken::SurfaceRaised)
        )),
        "the agent's row draws no band of its own"
    );

    // The text origins: the marker is the only difference.
    let reply_x = text_x(&reply, "Salut!");
    let message_x = text_x(&message, "Salut!");
    approx(
        "the agent's text keeps the band inset",
        reply_x,
        chat_typography::BAND_PADDING_X,
    );
    approx(
        "the user's text starts one marker and one gap past the same inset",
        message_x,
        chat_typography::BAND_PADDING_X
            + chat_typography::PROMPT_ARROW_SIZE
            + chat_typography::PROMPT_ARROW_GAP,
    );

    // No right-aligned run: every painted text ends inside the row's own
    // right inset, and no run starts near the pane's right edge.
    for command in reply.iter().chain(message.iter()) {
        if let RenderCommand::DrawText {
            origin,
            text,
            font_size,
            font_weight,
            font_family,
            font_italic,
            ..
        } = command
        {
            let advance = crate::platform::text_atlas::advance_width(
                text,
                *font_size,
                *font_weight,
                *font_family,
                *font_italic,
            );
            assert!(
                origin.x + advance <= width - chat_typography::BAND_PADDING_X + 0.5,
                "{text:?} is painted past the row's right inset"
            );
        }
    }

    // The marker belongs to the user's row only.
    assert!(
        message.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "prompt-arrow"
        )),
        "the user's row keeps its marker"
    );
    assert!(
        !reply.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "prompt-arrow"
        )),
        "the agent's row draws no marker"
    );
}

/// §6.3: Settings stay the source of truth. Every asserted ink is the theme's
/// resolved token — an Appearance override, a palette switch and a radius
/// change all move the chat with it — and the family seam reads `theme.font`.
#[test]
fn settings_remain_the_source_of_truth() {
    let width = 600.0;

    // Appearance overrides: both ink channels are read through the theme.
    let primary = ColorU::new(10, 20, 30, 255);
    let secondary = ColorU::new(200, 180, 160, 255);
    let mut app = AppContext::default();
    app.theme = Theme::dark().with_overrides(Some(primary), Some(secondary), None, None, None);
    let (commands, _) = fragments_commands(
        &app,
        ChatRole::Assistant,
        vec![ChatFragment::text("overridden prose")],
        width,
    );
    assert_eq!(
        runmatching(&commands, "overridden").2,
        primary,
        "the prose ink follows the Appearance override"
    );
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new()).with_tool_calls(vec![
        call(
            "credentials",
            r#"{"agent_id":"researcher"}"#,
            ToolCallStatus::Finished,
            Some("none"),
        ),
    ]);
    let commands = paint_bubble_at(&mut bubble, &app, width);
    assert_eq!(
        runmatching(&commands, "credentials").2,
        secondary,
        "the process-row ink follows the Appearance override"
    );

    // Palette switch: light and a preset resolve their own tokens.
    for theme in [Theme::light(), Theme::preset(ThemePreset::TokyoNight)] {
        let mut app = AppContext::default();
        app.theme = theme;
        let (commands, _) = fragments_commands(
            &app,
            ChatRole::Assistant,
            vec![ChatFragment::text("palette prose")],
            width,
        );
        assert_eq!(
            runmatching(&commands, "palette").2,
            app.theme.color(ColorToken::Text),
            "the prose adopts the switched palette"
        );
        let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new()).with_tool_calls(
            vec![call(
                "credentials",
                r#"{"agent_id":"researcher"}"#,
                ToolCallStatus::Finished,
                Some("none"),
            )],
        );
        let commands = paint_bubble_at(&mut bubble, &app, width);
        assert_eq!(
            runmatching(&commands, "credentials").2,
            app.theme.color(ColorToken::Muted),
            "the quiet rows adopt the switched palette"
        );
    }

    // The family seam: a theme family change reaches chat prose, while code and
    // tool bodies stay on the mono contract.
    let mut app = AppContext::default();
    app.theme = Theme::dark();
    app.theme.font = FontFamily::Mono;
    let (commands, _) = fragments_commands(
        &app,
        ChatRole::Assistant,
        vec![ChatFragment::text("family prose")],
        width,
    );
    assert_eq!(
        runmatching(&commands, "family").4,
        FontFamily::Mono,
        "chat prose reads the theme's family through chat_font()"
    );
    let (commands, _) = fragments_commands(
        &app,
        ChatRole::Assistant,
        vec![ChatFragment::code_block(Some("rust".into()), "let x = 1;")],
        width,
    );
    assert_eq!(
        runmatching(&commands, "let").4,
        FontFamily::Mono,
        "code stays mono whatever the theme family is"
    );

    // Radii are derived: a Settings radius change moves the DSH card radii.
    let mut app = AppContext::default();
    app.theme = Theme::dark();
    app.theme.radius = crate::theme::Radius::Rounded;
    approx(
        "announcement radius follows the theme",
        chat_typography::announcement_radius(&app),
        14.0 + chat_typography::ANNOUNCEMENT_RADIUS_EXTRA,
    );
    approx(
        "card radius follows the theme",
        chat_typography::card_radius(&app),
        14.0 + chat_typography::CARD_RADIUS_EXTRA,
    );

    // Density moves spacing, never type: DSH has no density axis, so "same
    // size as DSH" has to hold at every density setting.
    let mut sizes = Vec::new();
    for density in [Density::Compact, Density::Default, Density::Spacious] {
        let mut app = AppContext::default();
        app.theme = Theme::dark();
        app.theme.density = density;
        let (commands, _) = fragments_commands(
            &app,
            ChatRole::Assistant,
            vec![ChatFragment::text("density prose")],
            width,
        );
        let (size, line, ..) = runmatching(&commands, "density");
        sizes.push((size, line));
    }
    assert!(
        sizes.iter().all(|(size, line)| (*size - 14.0).abs() < 1e-6
            && (*line - chat_typography::BODY_LH).abs() < 1e-6),
        "density must not scale chat type, got {sizes:?}"
    );
}

/// §6.3.3: chat type is laid out in logical px, so the Settings "Font size"
/// zoom (a render scale) keeps scaling every surface. The elements never read
/// the zoom themselves — nothing bakes a physical pixel size.
#[test]
fn chat_type_is_logical_px_so_the_settings_zoom_scales_it() {
    let width = 600.0;
    let mut sizes = Vec::new();
    for zoom in [1.0f32, 1.25, 2.0] {
        let app = AppContext {
            ui_zoom: Rc::new(RefCell::new(zoom)),
            ..AppContext::default()
        };
        let (commands, row_size) = fragments_commands(
            &app,
            ChatRole::Assistant,
            vec![ChatFragment::text("zoom prose")],
            width,
        );
        let (size, line, ..) = runmatching(&commands, "zoom");
        sizes.push((row_size.x, size, line));
    }
    assert!(
        sizes.iter().all(|(row, size, line)| (*row - width).abs() < 0.01
            && (*size - 14.0).abs() < 1e-6
            && (*line - chat_typography::BODY_LH).abs() < 1e-6),
        "the layout stays in logical px and the render pass applies the zoom, got {sizes:?}"
    );
}

/// §6.3.5: no demo data. A fresh transcript with no messages draws no text and
/// no announcement row — the widget never seeds sample content.
#[test]
fn a_fresh_transcript_emits_no_sample_rows() {
    let app = AppContext::default();
    let message = ChatMessage::new(ChatRole::Assistant, Vec::new());
    let mut bubble = ChatMessageBubble::new(message.role, message.fragments);
    let commands = paint_bubble(&mut bubble, &app);
    assert!(
        commands.is_empty(),
        "an empty message paints nothing, got {commands:?}"
    );
}

/// The layout a bubble takes at `width`: its size and paint commands.
fn bubble_frame(
    app: &AppContext,
    role: ChatRole,
    fragments: Vec<ChatFragment>,
    calls: Vec<ToolCall>,
    fold: HashMap<String, ToolDisplayMode>,
    width: f32,
) -> (Vec<RenderCommand>, Vector2F) {
    let mut element: Box<dyn Element> = Box::new(
        ChatMessageBubble::new(role, fragments)
            .with_tool_calls(calls)
            .with_tool_fold(Rc::new(RefCell::new(fold))),
    );
    let commands = render_element(&mut element, vec2f(width, 1200.0), app);
    let size = element.size().expect("the row is laid out");
    (commands, size)
}

/// The first drawn run containing `needle`.
fn run_containing(commands: &[RenderCommand], needle: &str) -> (Vector2F, f32, f32) {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text,
                font_size,
                line_height,
                ..
            } if text.contains(needle) => Some((*origin, *font_size, *line_height)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the run {needle:?} is drawn, got {commands:?}"))
}

/// A row's words' baseline equals its leading mark's baseline: the mark is a
/// text run in the same 16 px cell painted at the row's top, so it must not be
/// shifted by a padding. A tool row is DSH's 24 px high.
#[test]
fn a_tool_row_is_twenty_four_high_and_its_mark_shares_the_baseline() {
    let app = AppContext::default();
    let call = call(
        "credentials",
        r#"{"agent_id":"researcher"}"#,
        ToolCallStatus::Finished,
        Some("none"),
    );
    let fold = HashMap::from([(tool_fold_key(&call, 0), ToolDisplayMode::Collapsed)]);
    let (commands, size) = bubble_frame(
        &app,
        ChatRole::Assistant,
        Vec::new(),
        vec![call],
        fold,
        400.0,
    );

    // The band's own 10 px inset over and under a single 24 px row.
    approx(
        "a single tool row's height",
        size.y - 2.0 * chat_typography::BAND_PADDING_Y,
        24.0,
    );

    let (mark_origin, mark_size, mark_lh) = run_containing(&commands, "◆");
    let (words_origin, words_size, words_lh) = run_containing(&commands, "credentials");
    approx("the mark's font size", mark_size, 13.0);
    approx("the words' font size", words_size, 13.0);
    approx(
        "the mark's line box is the 16 px cell",
        mark_size * mark_lh,
        chat_typography::LEADING_BOX,
    );
    approx("the words' line box is DSH's 24 px", words_size * words_lh, 24.0);
    approx(
        "the mark and the words share a baseline (same top, same size)",
        mark_origin.y,
        words_origin.y,
    );
}

/// A prose block gap is DSH's 16 px, and a heading carries its 32 px top
/// margin (the gap plus the remainder) over the block above it.
#[test]
fn prose_blocks_are_sixteen_apart_and_headings_carry_their_margin() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "first paragraph\n\nsecond paragraph\n\n## Heading\n\nafter the heading\n\n#### Fourth\n\nafter the fourth\n",
    );
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    let commands = paint_bubble_at(&mut bubble, &app, 600.0);

    let y = |needle: &str| run_containing(&commands, needle).0.y;
    let first = y("first paragraph");
    let second = y("second paragraph");
    let heading = y("Heading");
    let after = y("after the heading");
    let fourth = y("Fourth");
    let after_fourth = y("after the fourth");

    // A paragraph's line box is 24; DSH's block gap is 16.
    approx("paragraph to paragraph", second - first, 24.0 + 16.0);
    // H2 sits 32 below the previous block: the gap plus 16 more, then its own
    // 28 px line box starts.
    approx("paragraph to h2", heading - second, 24.0 + 16.0 + 16.0);
    // H2's bottom margin is the 16 px gap; its line box is 28.
    approx("h2 to the next paragraph", after - heading, 28.0 + 16.0);
    // h4's margins are the gap alone: 16 either side, its line box 24.
    approx("h2 paragraph to h4", fourth - after, 24.0 + 16.0);
    approx("h4 to the next paragraph", after_fourth - fourth, 24.0 + 16.0);
}

/// The fence's language banner draws on DSH's 18 px banner line, while the
/// body keeps the 19 px code line.
#[test]
fn the_code_banner_has_its_own_line_height() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(ChatRole::Assistant, "```rust\nlet x = 1;\n```");
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    let commands = paint_bubble_at(&mut bubble, &app, 600.0);

    let (_, banner_size, banner_lh) = run_containing(&commands, "rust");
    approx("the banner size", banner_size, 11.0);
    approx(
        "the banner line",
        banner_size * banner_lh,
        18.0,
    );
    let (_, body_size, body_lh) = run_containing(&commands, "let");
    approx("the body size", body_size, 11.0);
    approx("the body line", body_size * body_lh, 19.0);
}

/// The expanded body sits 4 px under the row that names it, inside the card.
#[test]
fn the_tool_body_card_sits_four_px_under_its_row() {
    let app = AppContext::default();
    let call = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}\n"),
    );
    let fold = HashMap::from([(tool_fold_key(&call, 0), ToolDisplayMode::Expanded)]);
    let (commands, _) = bubble_frame(
        &app,
        ChatRole::Assistant,
        Vec::new(),
        vec![call],
        fold,
        600.0,
    );
    let words_y = run_containing(&commands, "Read").0.y;
    let card_radius = chat_typography::card_radius(&app);
    let card = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - card_radius).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the expanded body is drawn on a card");
    approx(
        "the card starts 4 px under the row's 24 px line",
        card.min_y() - words_y,
        24.0 + 4.0,
    );
}

/// The reasoning disclosure title brightens from the quiet ink to the primary
/// ink while the pointer is over it — the hover swap DSH's row has.
#[test]
fn the_reasoning_title_has_hover_ink() {
    let app = AppContext::default();
    let mut bubble = ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::reasoning("c1:0", "done", "weighing it", true)],
    );
    let resting = paint_bubble(&mut bubble, &app);
    let resting_title = resting
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                text, color, ..
            } if text == "Thought" => Some(*color),
            _ => None,
        })
        .expect("the reasoning title is drawn");
    assert_eq!(
        resting_title,
        app.theme.color(ColorToken::Muted),
        "a resting reasoning row reads quiet"
    );

    let hovered = paint_with_pointer_over(&mut bubble, &app, "Thought");
    let hovered_title = hovered
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                text, color, ..
            } if text == "Thought" => Some(*color),
            _ => None,
        })
        .expect("the hovered reasoning title is drawn");
    assert_eq!(
        hovered_title,
        app.theme.color(ColorToken::Text),
        "the reasoning title brightens under the pointer"
    );
}
