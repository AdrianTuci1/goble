use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::color::ColorU;
use crate::elements::AppContext;
use crate::elements::Element;
use crate::elements::LayoutContext;
use crate::elements::PaintContext;
use crate::elements::SizeConstraint;
use crate::event::DispatchedEvent;
use crate::geometry::vec2f;
use crate::render::Renderer;
use crate::theme::ColorToken;

fn app() -> AppContext {
    AppContext::default()
}

#[test]
fn terminal_block_measures_non_zero() {
    let app = app();
    let mut block = TerminalBlock::new()
        .with_title("npm run build")
        .with_line(TerminalLine::command("npm run build"))
        .with_line(TerminalLine::output("Compiled successfully in 1.2s"))
        .with_status(TerminalStatus::Success);
    let size = block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);
    assert!(size.x <= 600.0);
}

#[test]
fn a_block_is_a_section_with_a_separator_and_no_card() {
    let app = app();
    let mut block = TerminalBlock::new()
        .with_title("zsh")
        .with_line(TerminalLine::command("cargo test"))
        .with_line(TerminalLine::success("test result: ok. 42 passed"))
        .with_line(TerminalLine::error("warning: unused variable"))
        .with_status(TerminalStatus::Running);
    let size = block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    // Full width: the section is the pane's own row, not a panel sized to its
    // content.
    assert_eq!(size.x, 600.0, "a block spans the width it is given");
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(10.0, 20.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    // The separator: a square full-width rule over the section.
    assert!(
        commands.iter().any(|c| matches!(
            c,
            crate::render::RenderCommand::FillRect { rect, corner_radius, .. }
                if *corner_radius == 0.0 && rect.width() == size.x
        )),
        "the section should open with a full-width separator"
    );
    // No card: no border and no rounded fill anywhere in the block.
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, crate::render::RenderCommand::StrokeRect { .. })),
        "a block draws no border"
    );
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            crate::render::RenderCommand::FillRect { corner_radius, .. } if *corner_radius > 0.0
        )),
        "a block draws no rounded card"
    );
    // The header shows the block identity (no icon, no status label).
    let text: Vec<&String> = commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert!(
        text.iter().any(|t| t.as_str() == "zsh"),
        "the header should draw the block title, got {text:?}"
    );
    assert!(
        text.iter().any(|t| t.as_str() == "cargo test"),
        "the command line should be drawn, got {text:?}"
    );
    assert!(
        text.iter()
            .any(|t| t.as_str() == "warning: unused variable"),
        "the error line should be drawn, got {text:?}"
    );
}

#[test]
fn terminal_copy_fires_with_block_text() {
    let app = app();
    let copied = Rc::new(RefCell::new(String::new()));
    let copied_clone = copied.clone();
    // The copy control is the block's own, so it is drawn only while the pointer
    // is over the block: the flag is the app-owned one the pointer move sets.
    let filter = TerminalFilter::default();
    filter.set_hovered(true);
    let mut block = TerminalBlock::new()
        .with_title("zsh")
        .with_line(TerminalLine::command("ls"))
        .with_line(TerminalLine::output("file.txt"))
        .with_filter(filter)
        .with_on_copy(move |text| *copied_clone.borrow_mut() = text);
    block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    // Compute the copy button's center (right-aligned, left of the filter
    // button) from the icon it draws, so the click lands on the button wherever
    // the header sits under the section's separator.
    let copy_center = commands
        .iter()
        .find_map(|c| match c {
            crate::render::RenderCommand::DrawIcon {
                origin, name, size, ..
            } if name == "copy" => Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0)),
            _ => None,
        })
        .expect("the header draws the copy button's icon");
    let mut event_ctx = crate::elements::EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position: copy_center,
        button: 0,
    };
    let up = DispatchedEvent::MouseUp {
        position: copy_center,
        button: 0,
    };
    assert!(block.dispatch_event(&down, &mut event_ctx, &app));
    assert!(block.dispatch_event(&up, &mut event_ctx, &app));
    let text = copied.borrow();
    assert!(
        text.contains("file.txt"),
        "copy should pass the block text, got {text:?}"
    );
}

#[test]
fn terminal_filter_hides_mismatched_lines() {
    let app = app();
    let filter = TerminalFilter::default();
    *filter.selected.borrow_mut() = 5; // Errors only
    let mut block = TerminalBlock::new()
        .with_title("zsh")
        .with_line(TerminalLine::command("cargo test"))
        .with_line(TerminalLine::success("ok"))
        .with_line(TerminalLine::error("boom"))
        .with_filter(filter);
    block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();
    let text: Vec<&String> = commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert!(
        !text.iter().any(|t| t.as_str() == "cargo test"),
        "filtered-out command line should not be drawn"
    );
    assert!(!text.iter().any(|t| t.as_str() == "ok"));
    assert!(
        text.iter().any(|t| t.as_str() == "boom"),
        "the matching error line should still be drawn"
    );
}

#[test]
fn terminal_global_filter_composes_with_block_filter() {
    let app = app();
    let global = TerminalFilter::default();
    *global.selected.borrow_mut() = 5; // Errors only
    let mut block = TerminalBlock::new()
        .with_title("zsh")
        .with_line(TerminalLine::command("cargo test"))
        .with_line(TerminalLine::output("compiling"))
        .with_line(TerminalLine::error("boom"))
        .with_global_filter(Some(global));
    block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();
    let text: Vec<&String> = commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert!(
        !text.iter().any(|t| t.as_str() == "cargo test"),
        "global Errors filter should hide the command line"
    );
    assert!(!text.iter().any(|t| t.as_str() == "compiling"));
    assert!(
        text.iter().any(|t| t.as_str() == "boom"),
        "the error line should still be drawn under the global filter"
    );
}

/// The rendered commands of a block, so a test can read the drawn runs.
fn paint_block(data: &TerminalData) -> Vec<crate::render::RenderCommand> {
    let app = app();
    let mut block = terminal_block(data, TerminalFilter::default(), None, None);
    crate::test_util::render_element(&mut block, vec2f(600.0, 300.0), &app)
}

/// Every drawn text run as `(text, colour)`.
fn draw_runs(commands: &[crate::render::RenderCommand]) -> Vec<(String, ColorU)> {
    commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, color, .. } => {
                Some((text.clone(), *color))
            }
            _ => None,
        })
        .collect()
}

/// H4: the command line is highlighted as shell — more than one run, in more
/// than one colour — rather than one flat row.
#[test]
fn command_line_is_highlighted_as_shell() {
    let data = TerminalData::for_command(
        "cargo test -p goble-ui -- --nocapture",
        "",
        TerminalStatus::Success,
    );
    let runs = draw_runs(&paint_block(&data));
    let full = "cargo test -p goble-ui -- --nocapture";
    let command_colors: Vec<ColorU> = runs
        .iter()
        .filter(|(text, _)| text != "❯ " && text != full)
        .map(|(_, color)| *color)
        .collect();
    assert!(
        command_colors.len() > 1,
        "the command line must draw as more than one run, got {runs:?}"
    );
    let distinct: std::collections::HashSet<ColorU> = command_colors.iter().copied().collect();
    assert!(
        distinct.len() > 1,
        "the command line must be more than one colour, got {command_colors:?}"
    );
}

/// H4: an output blob that emitted ANSI colours keeps them in the transcript.
#[test]
fn ansi_output_keeps_its_colours() {
    let data = TerminalData::for_command(
        "printf",
        "\x1b[31mred\x1b[0m and \x1b[32mgreen\x1b[0m",
        TerminalStatus::Success,
    );
    let runs = draw_runs(&paint_block(&data));
    let find = |want: &str| {
        runs.iter()
            .find(|(text, _)| text == want)
            .map(|(_, color)| *color)
    };
    let red = find("red").expect("the red run is drawn");
    let green = find("green").expect("the green run is drawn");
    assert_ne!(red, green, "red and green must not collapse to one colour");
    assert_ne!(
        red,
        app().theme.color(ColorToken::Text),
        "the coloured run must not be the default text colour"
    );
}

/// H4: a sequence the parser does not interpret costs no text — the words
/// around it survive, still carrying the colour in force.
#[test]
fn an_unrecognised_escape_sequence_loses_no_text() {
    let data = TerminalData::for_command(
        "run",
        "\x1b[31mhidden\x1b[10;20H visible\x1b[?9999z !",
        TerminalStatus::Success,
    );
    let runs = draw_runs(&paint_block(&data));
    let drawn = runs.iter().find(|(run, _)| run == "hidden visible !");
    let (_, color) = drawn.unwrap_or_else(|| panic!("no text may be dropped, got {runs:?}"));
    assert_ne!(
        *color,
        app().theme.color(ColorToken::Text),
        "the colour around the sequence must survive too, got {runs:?}"
    );
}

/// A section's header is one mono prompt row — where it ran, on which branch and
/// how long it took, the way a shell writes its prompt — drawn above the
/// command, which is drawn once on its own `❯` line.
#[test]
fn a_section_header_draws_one_prompt_row_above_the_command() {
    let app = app();
    let mut block = TerminalBlock::new()
        .with_title("")
        .with_line(TerminalLine::command("cargo test"))
        .with_line(TerminalLine::output("42 passed"))
        .with_meta(Some(
            TerminalMeta::new("~/Projects/goble")
                .with_branch("main")
                .with_duration("(1.24s)"),
        ))
        .with_status(TerminalStatus::Success);
    block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    let drawn = |text: &str| -> Option<(f32, ColorU, f32)> {
        commands.iter().find_map(|c| match c {
            crate::render::RenderCommand::DrawText {
                origin,
                text: t,
                color,
                font_size,
                ..
            } if t == text => Some((origin.y, *color, *font_size)),
            _ => None,
        })
    };

    // One row: the directory, the branch and the duration are one string, not
    // three labels.
    let prompt = "~/Projects/goble git:(main) (1.24s)";
    let (prompt_y, prompt_color, prompt_size) = drawn(prompt)
        .unwrap_or_else(|| panic!("the header draws one prompt row, got {commands:?}"));
    assert_eq!(
        prompt_color,
        app.theme.color(ColorToken::Muted),
        "the prompt row is muted"
    );
    assert_eq!(prompt_size, 13.0, "the prompt row is the block's mono size");

    let command = drawn("cargo test").expect("the section body draws the command");
    assert!(
        prompt_y < command.0,
        "the prompt row sits at the top of the section"
    );
    for separate in ["~/Projects/goble", "git:(main)", "(1.24s)"] {
        assert!(
            !commands.iter().any(|c| matches!(
                c,
                crate::render::RenderCommand::DrawText { text, .. } if text == separate
            )),
            "the prompt row is one row, not a label per part: {separate} drawn alone"
        );
    }
}

/// A block built from `data` and painted through `filter`, with `global` as the
/// whole-surface filter when one is wired.
fn paint_through(
    data: &TerminalData,
    filter: TerminalFilter,
    global: Option<TerminalFilter>,
) -> Vec<crate::render::RenderCommand> {
    let app = app();
    let mut block = terminal_block(data, filter, global, None);
    crate::test_util::render_element(&mut block, vec2f(600.0, 300.0), &app)
}

/// Every drawn text, in draw order.
fn drawn_texts(commands: &[crate::render::RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Whether `commands` draw the named icon.
fn draws_icon(commands: &[crate::render::RenderCommand], name: &str) -> bool {
    commands.iter().any(
        |c| matches!(c, crate::render::RenderCommand::DrawIcon { name: icon, .. } if icon == name),
    )
}

/// Every filled rect as `(width, height, colour)`.
fn filled_rects(commands: &[crate::render::RenderCommand]) -> Vec<(f32, f32, ColorU)> {
    commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::FillRect { rect, color, .. } => {
                Some((rect.width(), rect.height(), *color))
            }
            _ => None,
        })
        .collect()
}

/// A section of four output lines, so a query has something to narrow.
fn section(lines: &[(&str, TerminalLineKind)]) -> TerminalData {
    TerminalData::new(
        "",
        lines
            .iter()
            .map(|(text, kind)| TerminalLine::styled(*text, *kind, Vec::new()))
            .collect(),
    )
}

/// A: the block's controls are its own, so they are drawn only while the
/// pointer is over the block; with the pointer away the header is only the
/// prompt row.
#[test]
fn the_blocks_controls_show_only_while_the_pointer_is_over_the_block() {
    let data = section(&[("cargo test", TerminalLineKind::Command)]);
    let away = paint_through(&data, TerminalFilter::default(), None);
    assert!(
        !draws_icon(&away, "copy") && !draws_icon(&away, "sliders"),
        "a block the pointer is not over draws no controls: {:?}",
        drawn_texts(&away)
    );

    let hovered = TerminalFilter::default();
    hovered.set_hovered(true);
    let over = paint_through(&data, hovered, None);
    assert!(
        draws_icon(&over, "copy") && draws_icon(&over, "sliders"),
        "the block under the pointer draws its copy and filter controls"
    );
}

/// A: the block's own hover flag is app-owned, so a pointer move writes it and
/// the next frame — a brand new element — reads it back. Only the block under
/// the pointer is marked.
#[test]
fn the_pointer_over_a_block_sets_that_blocks_hover_flag() {
    let app = app();
    let under = TerminalFilter::default();
    let elsewhere = TerminalFilter::default();
    let mut block = TerminalBlock::new()
        .with_line(TerminalLine::output("first"))
        .with_filter(under.clone());
    let size = block.layout(
        SizeConstraint::loose(vec2f(600.0, 300.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let mut event_ctx = crate::elements::EventContext::default();

    let inside = vec2f(10.0, size.y / 2.0);
    block.dispatch_event(
        &DispatchedEvent::MouseMove { position: inside },
        &mut event_ctx,
        &app,
    );
    assert!(under.is_hovered(), "the pointer in the block hovers it");
    assert!(
        !elsewhere.is_hovered(),
        "a pointer over one block does not hover another"
    );

    let outside = vec2f(900.0, 900.0);
    block.dispatch_event(
        &DispatchedEvent::MouseMove { position: outside },
        &mut event_ctx,
        &app,
    );
    assert!(!under.is_hovered(), "the pointer away un-hovers it");

    // The pointer inside again: the controls are drawn by the next frame, which
    // is a new block reading the same app-owned flag.
    block.dispatch_event(
        &DispatchedEvent::MouseMove { position: inside },
        &mut event_ctx,
        &app,
    );
    let next_frame = paint_through(
        &section(&[("first", TerminalLineKind::Output)]),
        under,
        None,
    );
    assert!(
        draws_icon(&next_frame, "copy"),
        "the flag the pointer set is what the next frame's controls are drawn from"
    );
}

/// A: a failed block is washed in the error colour and stands a pole on its left
/// edge; a running one keeps the pole in the accent colour; a successful block
/// draws neither.
#[test]
fn a_failed_block_is_washed_and_flagged_in_the_error_colour() {
    let app = app();
    let error = app.theme.color(ColorToken::Error);
    let accent = app.theme.color(ColorToken::Accent);

    let failed = paint_through(
        &section(&[("boom", TerminalLineKind::Error)]).with_status(TerminalStatus::Error),
        TerminalFilter::default(),
        None,
    );
    let fills = filled_rects(&failed);
    let wash = fills
        .iter()
        .find(|(width, _, color)| {
            *width == 600.0 && color.r == error.r && color.g == error.g && color.b == error.b
        })
        .unwrap_or_else(|| panic!("a failure washes the whole block, got {fills:?}"));
    assert!(
        wash.2.a > 0 && wash.2.a < 255,
        "the wash is a tint over the pane, not a solid fill: {:?}",
        wash.2
    );
    let pole = fills
        .iter()
        .find(|(width, _, color)| *width == 3.0 && *color == error)
        .unwrap_or_else(|| panic!("a failure stands a pole in the error colour, got {fills:?}"));
    assert_eq!(
        pole.1, wash.1,
        "the pole runs the whole height of the washed block"
    );

    let error_rgb = |color: ColorU| color.r == error.r && color.g == error.g && color.b == error.b;
    let running = paint_through(
        &section(&[("sleep 30", TerminalLineKind::Command)]).with_status(TerminalStatus::Running),
        TerminalFilter::default(),
        None,
    );
    let running_fills = filled_rects(&running);
    assert!(
        running_fills
            .iter()
            .any(|(width, _, color)| *width == 3.0 && *color == accent),
        "a running block stands an accent pole: {running_fills:?}"
    );
    assert!(
        !running_fills.iter().any(|(_, _, color)| error_rgb(*color)),
        "a running block is not washed in the error colour: {running_fills:?}"
    );

    let done = paint_through(
        &section(&[("ls", TerminalLineKind::Command)]).with_status(TerminalStatus::Success),
        TerminalFilter::default(),
        None,
    );
    let done_fills = filled_rects(&done);
    assert!(
        !done_fills.iter().any(|(width, _, _)| *width == 3.0),
        "a finished block draws no pole: {done_fills:?}"
    );
    assert!(
        !done_fills.iter().any(|(_, _, color)| error_rgb(*color)),
        "a finished block is not washed in the error colour: {done_fills:?}"
    );
}

/// B: the filter button opens a bar with a real text input over the block's own
/// output — a placeholder field with its own caret and the count of lines the
/// query leaves.
#[test]
fn the_filter_button_opens_a_bar_with_a_real_text_field() {
    let app = app();
    let filter = TerminalFilter::default();
    filter.set_hovered(true);
    let data = section(&[
        ("cargo test", TerminalLineKind::Command),
        ("alpha", TerminalLineKind::Output),
        ("beta", TerminalLineKind::Output),
    ]);
    let mut block = terminal_block(&data, filter.clone(), None, None);
    let commands = crate::test_util::render_element(&mut block, vec2f(600.0, 300.0), &app);
    let button = commands
        .iter()
        .find_map(|c| match c {
            crate::render::RenderCommand::DrawIcon {
                origin, name, size, ..
            } if name == "sliders" => Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0)),
            _ => None,
        })
        .expect("the hovered block draws its filter button");
    let mut event_ctx = crate::elements::EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: button,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: button,
            button: 0,
        },
    ] {
        block.dispatch_event(&event, &mut event_ctx, &app);
    }
    assert!(filter.is_open(), "the button shows the block's filter bar");
    assert!(
        *filter.focused.borrow(),
        "the bar's field is ready to type in"
    );

    let next_frame = paint_through(&data, filter.clone(), None);
    let texts = drawn_texts(&next_frame);
    assert!(
        texts.iter().any(|t| t == "Filter block output"),
        "an empty bar draws its placeholder: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "3 of 3"),
        "the bar counts the lines the query leaves: {texts:?}"
    );
    assert!(
        next_frame.iter().any(|c| matches!(
            c,
            crate::render::RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Focus)
                    && (rect.width() - crate::elements::caret::CARET_WIDTH).abs() < 0.5
                    && (rect.height() - crate::elements::caret::CARET_HEIGHT).abs() < 0.5
        )),
        "the focused field draws its caret beam"
    );
}

/// B: what is typed into the bar narrows that block's lines, and Escape puts the
/// whole output back.
#[test]
fn typing_in_the_bar_narrows_the_block_and_escape_restores_it() {
    let app = app();
    let filter = TerminalFilter::default();
    filter.set_hovered(true);
    filter.open_bar();
    let data = section(&[
        ("cargo test", TerminalLineKind::Command),
        ("alpha line", TerminalLineKind::Output),
        ("beta line", TerminalLineKind::Output),
    ]);
    let mut block = terminal_block(&data, filter.clone(), None, None);
    let _ = crate::test_util::render_element(&mut block, vec2f(600.0, 300.0), &app);

    let mut event_ctx = crate::elements::EventContext::default();
    for key in ["b", "E", "t", "a"] {
        assert!(
            block.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app
            ),
            "the bar's field takes the keystroke {key:?}"
        );
    }
    assert_eq!(
        filter.query.borrow().as_str(),
        "bEta",
        "the field's query is the app-owned one"
    );

    // The next frame draws only the line the query keeps, and counts it.
    let narrowed = paint_through(&data, filter.clone(), None);
    let texts = drawn_texts(&narrowed);
    assert!(
        texts.iter().any(|t| t == "beta line"),
        "the matching line is drawn: {texts:?}"
    );
    for hidden in ["alpha line", "cargo test"] {
        assert!(
            !texts.iter().any(|t| t == hidden),
            "{hidden} does not match and is not drawn: {texts:?}"
        );
    }
    assert!(
        texts.iter().any(|t| t == "1 of 3"),
        "the bar counts what the query left: {texts:?}"
    );

    // Escape: the bar goes and the block draws its whole output again.
    let mut block = terminal_block(&data, filter.clone(), None, None);
    let _ = crate::test_util::render_element(&mut block, vec2f(600.0, 300.0), &app);
    assert!(block.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "Escape".to_string(),
            modifiers: Default::default(),
        },
        &mut event_ctx,
        &app
    ));
    assert!(!filter.is_open(), "Escape closes the bar");
    assert_eq!(
        filter.query.borrow().as_str(),
        "",
        "and drops the query with it"
    );
    let restored = drawn_texts(&paint_through(&data, filter, None));
    for line in ["alpha line", "beta line", "cargo test"] {
        assert!(
            restored.iter().any(|t| t == line),
            "the whole output is back: {restored:?}"
        );
    }
}

/// B: a query is the block's own — another block, with its own filter state,
/// draws every line.
#[test]
fn a_query_in_one_block_leaves_another_block_alone() {
    let filter = TerminalFilter::default();
    filter.set_query("beta");
    let mine = TerminalData::new(
        "first",
        vec![TerminalLine::output("alpha"), TerminalLine::output("beta")],
    );
    let theirs = TerminalData::new(
        "second",
        vec![TerminalLine::output("alpha"), TerminalLine::output("beta")],
    );

    let narrowed = drawn_texts(&paint_through(&mine, filter, None));
    assert!(
        narrowed.iter().any(|t| t == "beta") && !narrowed.iter().any(|t| t == "alpha"),
        "the queried block keeps only its matching lines: {narrowed:?}"
    );

    let untouched = drawn_texts(&paint_through(&theirs, TerminalFilter::default(), None));
    assert!(
        untouched.iter().any(|t| t == "alpha") && untouched.iter().any(|t| t == "beta"),
        "a block with its own filter state is untouched: {untouched:?}"
    );
}

/// C: one whole-surface filter narrows every block it is handed, and its query
/// matches case-insensitively — the same surface the Cmd+F bar opens.
#[test]
fn the_general_filter_narrows_every_block_it_is_handed() {
    let general = TerminalFilter::default();
    assert!(!terminal_filter_open(&general), "it starts closed");
    toggle_terminal_filter(&general);
    assert!(
        terminal_filter_open(&general),
        "the chord's toggle opens it"
    );
    general.set_query("BETA");

    let first = TerminalData::new(
        "first",
        vec![
            TerminalLine::output("alpha"),
            TerminalLine::output("beta one"),
        ],
    );
    let second = TerminalData::new(
        "second",
        vec![
            TerminalLine::output("beta two"),
            TerminalLine::output("gamma"),
        ],
    );

    let first_texts = drawn_texts(&paint_through(
        &first,
        TerminalFilter::default(),
        Some(general.clone()),
    ));
    assert!(
        first_texts.iter().any(|t| t == "beta one") && !first_texts.iter().any(|t| t == "alpha"),
        "the general filter narrows the first block: {first_texts:?}"
    );
    let second_texts = drawn_texts(&paint_through(
        &second,
        TerminalFilter::default(),
        Some(general.clone()),
    ));
    assert!(
        second_texts.iter().any(|t| t == "beta two") && !second_texts.iter().any(|t| t == "gamma"),
        "and the second one: {second_texts:?}"
    );

    // Closing it puts both blocks back.
    toggle_terminal_filter(&general);
    let restored = drawn_texts(&paint_through(
        &first,
        TerminalFilter::default(),
        Some(general),
    ));
    assert!(
        restored.iter().any(|t| t == "alpha"),
        "a closed general filter hides nothing: {restored:?}"
    );
}

/// C: the general bar is the same surface as a block's — a real field, the
/// app-owned query and the counts of every block the surface drew.
#[test]
fn the_general_filter_bar_is_a_real_field_with_the_counts() {
    let app = app();
    let filter = TerminalFilter::default();
    filter.open_bar();
    let mut bar = terminal_filter_bar(&filter, "Filter terminal output", 3, 12, &app);
    let commands = crate::test_util::render_element(&mut bar, vec2f(600.0, 40.0), &app);
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "Filter terminal output"),
        "the bar names what it filters: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "3 of 12"),
        "the bar counts the matches over every block: {texts:?}"
    );

    let mut event_ctx = crate::elements::EventContext::default();
    for key in ["o", "k"] {
        assert!(
            bar.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app
            ),
            "the bar's field takes the keystroke {key:?}"
        );
    }
    assert_eq!(
        filter.query.borrow().as_str(),
        "ok",
        "the general bar types into the same app-owned query"
    );
}
