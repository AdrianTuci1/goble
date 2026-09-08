//! A full-pane image view, used to render a live screen stream (broadcast or a
//! remote xrdp desktop inside a pane).
//!
//! The element holds the most recent RGBA8 frame for a source and paints it
//! scaled to fill the pane. The app owns the frame in state (it survives the
//! per-frame tree rebuild); the element only borrows the current `Arc<[u8]>`
//! and the monotonic frame sequence, so an unchanged frame never re-uploads.

use std::sync::Arc;

use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::Vector2F;

/// The dimensions of the most recent source frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

/// A live source frame: raw RGBA8 pixels plus the parity that indicates the
/// pixels actually changed (`frame_seq`). Drawn scaled to the pane bounds.
pub struct FrameView {
    size: Option<Vector2F>,
    origin: Option<Point>,
    source: String,
    frame_seq: u64,
    data: Arc<[u8]>,
    width: u32,
    height: u32,
}

impl FrameView {
    /// A new `FrameView` for `source` showing `data` (RGBA8, `width*height*4`
    /// bytes). `frame_seq` is an opaque monotonic value used to detect pixel
    /// changes; duplicate frames are expected to reuse the same value.
    pub fn new(
        source: impl Into<String>,
        frame_seq: u64,
        width: u32,
        height: u32,
        data: Arc<[u8]>,
    ) -> Self {
        Self {
            size: None,
            origin: None,
            source: source.into(),
            frame_seq,
            data,
            width,
            height,
        }
    }

    /// The source frame dimensions, for aspect-ratio-aware callers.
    pub fn frame_size(&self) -> FrameSize {
        FrameSize {
            width: self.width,
            height: self.height,
        }
    }
}

impl Element for FrameView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = Vector2F::new(constraint.width(), constraint.height());
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = match self.size {
            Some(s) => s,
            None => return,
        };
        let rect = crate::geometry::rectf(origin.x, origin.y, size.x, size.y);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.draw_image(
                rect,
                self.source.clone(),
                self.width,
                self.height,
                self.frame_seq,
                Arc::clone(&self.data),
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, LayoutContext, SizeConstraint};
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};
    use crate::test_util::command_counts;

    #[test]
    fn frame_view_emits_draw_image_command() {
        let app = AppContext::default();
        let pixels = vec![7u8; 4 * 4 * 4];
        let mut element = FrameView::new("remote-xrdp", 3, 4, 4, Arc::from(pixels));

        let mut layout_ctx = LayoutContext::default();
        let _ = element.layout(SizeConstraint::loose(vec2f(200.0, 150.0)), &mut layout_ctx, &app);

        let renderer = Renderer::new();
        let mut paint_ctx = crate::elements::PaintContext::new(renderer);
        element.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx.renderer.take().unwrap().commands().to_vec();

        let counts = command_counts(&commands);
        assert_eq!(counts.draw_image, 1, "frame view should emit one image draw");

        let draw = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::DrawImage { source, width, height, .. } => {
                    Some((source.as_str(), *width, *height))
                }
                _ => None,
            })
            .expect("a DrawImage command");
        assert_eq!(draw, ("remote-xrdp", 4, 4));
    }
}
