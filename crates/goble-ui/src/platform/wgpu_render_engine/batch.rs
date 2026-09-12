use crate::render::RenderCommand;

/// The vertex buffer a render command contributes its geometry to. A batch
/// draws one kind after another (rects, then text, then icons, then images), so
/// a command whose kind differs from the previous one has to close the batch;
/// otherwise a panel could not cover a label painted before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Primitive {
    Rect,
    Text,
    Icon,
    Image,
}

/// Returns the primitive that `command` starts a new batch with, or `None` when
/// it continues the current batch (same kind) or only changes the clip stack.
pub(super) fn starts_new_batch(
    current: Option<Primitive>,
    command: &RenderCommand,
) -> Option<Primitive> {
    let kind = match command {
        RenderCommand::FillRect { .. }
        | RenderCommand::FillRectFadeRight { .. }
        | RenderCommand::StrokeRect { .. } => Primitive::Rect,
        RenderCommand::DrawText { .. } => Primitive::Text,
        RenderCommand::DrawIcon { .. } => Primitive::Icon,
        RenderCommand::DrawImage { .. } => Primitive::Image,
        RenderCommand::ClipRect(_) | RenderCommand::PopClip => return None,
    };
    if current == Some(kind) {
        None
    } else {
        Some(kind)
    }
}

/// Compute the scissor rect (in physical pixels) from the active clip stack.
/// An empty stack clips to the full viewport.
pub(super) fn clip_scissor(
    clip_stack: &[crate::geometry::RectF],
    scale: f32,
    viewport: (u32, u32),
) -> (u32, u32, u32, u32) {
    let vw = viewport.0 as f32;
    let vh = viewport.1 as f32;
    let mut acc: Option<crate::geometry::RectF> = None;
    for r in clip_stack {
        acc = Some(match acc {
            None => *r,
            Some(a) => intersect(a, *r),
        });
    }
    let (x0, y0, x1, y1) = match acc {
        Some(r) => (
            r.origin.x * scale,
            r.origin.y * scale,
            (r.origin.x + r.size.width) * scale,
            (r.origin.y + r.size.height) * scale,
        ),
        None => (0.0, 0.0, vw, vh),
    };
    let sx = x0.max(0.0).min(vw);
    let sy = y0.max(0.0).min(vh);
    let ex = x1.max(x0).min(vw);
    let ey = y1.max(y0).min(vh);
    (
        sx as u32,
        sy as u32,
        (ex - sx).max(0.0) as u32,
        (ey - sy).max(0.0) as u32,
    )
}

fn intersect(a: crate::geometry::RectF, b: crate::geometry::RectF) -> crate::geometry::RectF {
    let x0 = a.origin.x.max(b.origin.x);
    let y0 = a.origin.y.max(b.origin.y);
    let x1 = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let y1 = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    crate::geometry::rectf(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}
