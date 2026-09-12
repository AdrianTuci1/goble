use crate::color::ColorU;
use crate::geometry::{rectf, vec2f};
use crate::render::{RenderCommand, Renderer};

use super::batch::starts_new_batch;

/// Mirrors the batching loop in [`Renderer`]'s engine: one batch per
/// primitive-kind change plus one per clip boundary.
fn batch_count(commands: &[RenderCommand]) -> usize {
    let mut count = 0;
    let mut current = None;
    for command in commands {
        if let Some(kind) = starts_new_batch(current, command) {
            count += 1;
            current = Some(kind);
        }
        if matches!(command, RenderCommand::ClipRect(_) | RenderCommand::PopClip) {
            count += 1;
        }
    }
    count
}

/// A batch draws all of its rects before any of its text/icons, so a panel
/// painted after a label must start a new batch or the label would draw
/// over it (a tray could never cover the surface below it).
#[test]
fn a_panel_painted_after_a_label_starts_a_new_batch() {
    let white = ColorU::new(255, 255, 255, 255);
    let mut renderer = Renderer::new();
    renderer.draw_icon(vec2f(0.0, 0.0), "plus", 12.0, white);
    renderer.fill_rect(rectf(0.0, 0.0, 10.0, 10.0), white);
    assert_eq!(
        batch_count(renderer.commands()),
        2,
        "the rect must not share a batch with the icon painted before it"
    );
}

/// Commands of the same kind keep accumulating into one batch.
#[test]
fn consecutive_same_kind_commands_share_a_batch() {
    let white = ColorU::new(255, 255, 255, 255);
    let mut renderer = Renderer::new();
    renderer.fill_rect(rectf(0.0, 0.0, 10.0, 10.0), white);
    renderer.fill_rect(rectf(0.0, 12.0, 10.0, 10.0), white);
    assert_eq!(batch_count(renderer.commands()), 1);
}

/// A clip boundary always closes a batch, and the kind flow continues after
/// it without an extra switch.
#[test]
fn a_clip_boundary_closes_a_batch_and_keeps_the_kind() {
    let white = ColorU::new(255, 255, 255, 255);
    let mut renderer = Renderer::new();
    renderer.fill_rect(rectf(0.0, 0.0, 10.0, 10.0), white);
    renderer.clip_rect(rectf(0.0, 0.0, 5.0, 5.0));
    renderer.fill_rect(rectf(0.0, 0.0, 5.0, 5.0), white);
    renderer.pop_clip();
    assert_eq!(
        batch_count(renderer.commands()),
        3,
        "rect, clipped rect, then the batch closed by the pop"
    );
}
