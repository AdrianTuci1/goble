use std::cell::RefCell;
use std::rc::Rc;

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
use super::*;

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
fn terminal_block_paints_background_header_and_lines() {
    let app = app();
    let mut block = TerminalBlock::new()
        .with_title("zsh")
        .with_line(TerminalLine::command("cargo test"))
        .with_line(TerminalLine::success("test result: ok. 42 passed"))
        .with_line(TerminalLine::error("warning: unused variable"))
        .with_status(TerminalStatus::Running);
    block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    block.paint(vec2f(10.0, 20.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, crate::render::RenderCommand::FillRect { .. })),
        "terminal block should paint a background"
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
        text.iter().any(|t| t.as_str() == "warning: unused variable"),
        "the error line should be drawn, got {text:?}"
    );
}

#[test]
fn terminal_copy_fires_with_block_text() {
    let app = app();
    let copied = Rc::new(RefCell::new(String::new()));
    let copied_clone = copied.clone();
    let mut block = TerminalBlock::new()
        .with_title("zsh")
        .with_line(TerminalLine::command("ls"))
        .with_line(TerminalLine::output("file.txt"))
        .with_on_copy(move |text| *copied_clone.borrow_mut() = text);
    block.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    block.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
    // Compute the copy button's center (right-aligned, left of the filter
    // button) and click it.
    let size = block.size.unwrap();
    let copy_center = vec2f(size.x - PADDING_X - BUTTON_SIZE - HEADER_SPACING - BUTTON_SIZE / 2.0, PADDING_Y + BUTTON_SIZE / 2.0);
    let mut event_ctx = crate::elements::EventContext::default();
    let down = DispatchedEvent::MouseDown { position: copy_center, button: 0 };
    let up = DispatchedEvent::MouseUp { position: copy_center, button: 0 };
    assert!(block.dispatch_event(&down, &mut event_ctx, &app));
    assert!(block.dispatch_event(&up, &mut event_ctx, &app));
    let text = copied.borrow();
    assert!(text.contains("file.txt"), "copy should pass the block text, got {text:?}");
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
    let commands = paint_ctx
        .renderer
        .take()
        .unwrap()
        .commands()
        .to_vec();
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
    let commands = paint_ctx
        .renderer
        .take()
        .unwrap()
        .commands()
        .to_vec();
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

/// A section's header carries the block's context — where it ran and how long
/// it took — above the command, which is drawn once on its own `❯` line.
#[test]
fn a_section_header_draws_the_path_branch_and_duration_above_the_command() {
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
    let drawn = |text: &str| -> Option<f32> {
        commands.iter().find_map(|c| match c {
            crate::render::RenderCommand::DrawText { origin, text: t, .. } if t == text => {
                Some(origin.y)
            }
            _ => None,
        })
    };

    let path = drawn("~/Projects/goble").expect("the section header draws the path");
    let branch = drawn("git:(main)").expect("the section header draws the branch");
    let duration = drawn("(1.24s)").expect("the section header draws the duration");
    let command = drawn("cargo test").expect("the section body draws the command");
    assert!(path < command, "the path sits at the top of the section");
    assert!(branch < command, "the branch sits at the top of the section");
    assert!(duration < command, "the duration sits at the top of the section");
}
