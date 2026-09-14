//! The last-painted layer that draws the frame's hover chips.
//!
//! The window is drawn from one element tree per frame and there is no
//! cross-tree z-index, so a chip drawn during the paint pass of the element it
//! belongs to is covered by anything painted after that element: a sidebar
//! toolbelt tab's chip is covered by the pane on its right. An element that
//! wants its chip above everything (a [`Tooltip`](super::tooltip::Tooltip))
//! queues the box in [`HoverChipRegistry`] instead of drawing it, and
//! [`HoverChipLayer`] — the last thing the root paints — draws the queued boxes
//! and empties the registry.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::Vector2F;

/// What a queued chip draws: a box the queuer hands over, or a box it keeps a
/// handle on.
///
/// The owned form is what a `Tooltip` queues — it has nothing more to do with
/// the box once it is drawn. The shared form is for a panel that stays
/// interactive: its owner keeps the same `Rc` to lay it out and to dispatch
/// clicks into it, while the layer draws it above everything.
enum ChipPanel {
    Owned(Box<dyn Element>),
    Shared(Rc<RefCell<Box<dyn Element>>>),
}

impl ChipPanel {
    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        match self {
            ChipPanel::Owned(panel) => panel.paint(origin, ctx, app),
            ChipPanel::Shared(panel) => panel.borrow_mut().paint(origin, ctx, app),
        }
    }
}

/// One chip queued for this frame: the box to draw and where its top-left
/// corner sits, in window coordinates.
struct HoverChip {
    origin: Vector2F,
    panel: ChipPanel,
}

/// The chips queued while painting the current frame.
///
/// Reached through `AppContext::hover_chips`, so the element that queues a chip
/// and the layer that draws it share one registry per app. The layer drains it
/// every time it paints, so a chip cannot survive into a later frame.
#[derive(Default)]
pub struct HoverChipRegistry {
    chips: Vec<HoverChip>,
}

impl HoverChipRegistry {
    /// Queue `panel` to be drawn above every other element, its top-left corner
    /// at `origin` in window coordinates. `panel` is laid out by its owner; the
    /// layer only draws it.
    pub fn push(&mut self, origin: Vector2F, panel: Box<dyn Element>) {
        self.chips.push(HoverChip {
            origin,
            panel: ChipPanel::Owned(panel),
        });
    }

    /// Queue a panel the owner keeps: the layer draws it above every other
    /// element, and the owner keeps the same handle to lay it out and to
    /// dispatch events into it (the layer draws, it does not dispatch).
    pub fn push_shared(&mut self, origin: Vector2F, panel: Rc<RefCell<Box<dyn Element>>>) {
        self.chips.push(HoverChip {
            origin,
            panel: ChipPanel::Shared(panel),
        });
    }

    /// How many chips this frame queued. Zero on a frame that hovered nothing.
    pub fn len(&self) -> usize {
        self.chips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chips.is_empty()
    }

    /// Draw every queued chip and empty the registry. Only [`HoverChipLayer`]
    /// calls this: it is the last paint of the frame, so the boxes it draws are
    /// above the whole tree.
    fn drain(&mut self, ctx: &mut PaintContext, app: &AppContext) {
        // Taken before the panels draw, so the frame leaves the registry empty
        // whether or not a panel itself paints.
        let chips = std::mem::take(&mut self.chips);
        for mut chip in chips {
            chip.panel.paint(chip.origin, ctx, app);
        }
    }
}

/// Draws the frame's queued hover chips, above every other element.
///
/// The root paints this element after its whole tree, so a chip is never
/// covered by the pane to the right of the element it belongs to, nor by an open
/// overlay. A frame with nothing hovered draws nothing.
pub struct HoverChipLayer {
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HoverChipLayer {
    pub fn new() -> Self {
        Self {
            size: None,
            origin: None,
        }
    }
}

impl Default for HoverChipLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for HoverChipLayer {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        // A chip is positioned in window coordinates by the element that queued
        // it, so the layer itself takes no room.
        self.size = Some(Vector2F::zero());
        Vector2F::zero()
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        app.hover_chips.borrow_mut().drain(ctx, app);
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
    use crate::color::ColorU;
    use crate::elements::{Container, Empty, Fill, Stack, Tooltip};
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};
    use crate::theme::ColorToken;

    /// The opaque panel a later sibling paints over the chip's own element.
    const COVER: ColorU = ColorU::new(9, 9, 9, 255);

    /// A tooltip-wrapped box, an opaque panel painted after it, and the layer
    /// the root paints last — the composition that used to hide the chip.
    fn covered_chip() -> Box<dyn Element> {
        let target = Tooltip::new(Empty::new().with_size(vec2f(40.0, 32.0)).finish(), "Run")
            .finish();
        let cover = Container::new(Empty::new().with_size(vec2f(200.0, 200.0)).finish())
            .with_background(Fill::Solid(COVER))
            .finish();
        Stack::new()
            .with_children(vec![target, cover, HoverChipLayer::new().finish()])
            .finish()
    }

    /// Lay out and paint one frame of `root`, with the pointer at `cursor`.
    fn paint_frame(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        cursor: Option<Vector2F>,
    ) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(position) = cursor {
            ctx.cursor_inside = true;
            ctx.cursor_position = position;
        }
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// Where the later sibling's cover is painted.
    fn cover_index(commands: &[RenderCommand]) -> usize {
        commands
            .iter()
            .position(
                |command| matches!(command, RenderCommand::FillRect { color, .. } if *color == COVER),
            )
            .expect("the covering panel paints")
    }

    /// Where the chip's own box is painted.
    fn chip_box_index(commands: &[RenderCommand], app: &AppContext) -> Option<usize> {
        let color = app.theme.color(ColorToken::SurfaceRaised);
        commands.iter().position(|command| {
            matches!(command, RenderCommand::FillRect { color: drawn, .. } if *drawn == color)
        })
    }

    /// Where the chip's message is painted.
    fn chip_text_index(commands: &[RenderCommand]) -> Option<usize> {
        commands.iter().position(
            |command| matches!(command, RenderCommand::DrawText { text, .. } if text == "Run"),
        )
    }

    #[test]
    fn a_hovered_chip_is_painted_after_the_sibling_that_covers_it() {
        let app = AppContext::default();
        let mut root = covered_chip();

        let commands = paint_frame(&mut root, &app, Some(vec2f(20.0, 16.0)));

        let cover = cover_index(&commands);
        let box_index = chip_box_index(&commands, &app).expect("the chip's box paints");
        let text = chip_text_index(&commands).expect("the chip's message paints");
        assert!(
            box_index > cover,
            "the chip's box is drawn after the panel covering its element (box {box_index}, cover {cover})"
        );
        assert!(
            text > cover,
            "the chip's message is drawn after the panel covering its element (text {text}, cover {cover})"
        );
        assert_eq!(
            text,
            commands.len() - 1,
            "the chip is the last thing the frame paints"
        );
    }

    #[test]
    fn a_frame_with_nothing_hovered_queues_nothing() {
        let app = AppContext::default();
        let mut root = covered_chip();

        // The pointer outside the window.
        let commands = paint_frame(&mut root, &app, None);
        assert!(chip_text_index(&commands).is_none(), "no chip is drawn");
        assert!(chip_box_index(&commands, &app).is_none(), "no chip is drawn");
        assert!(app.hover_chips.borrow().is_empty());

        // The pointer inside the window, but away from the tooltip's element.
        let commands = paint_frame(&mut root, &app, Some(vec2f(180.0, 180.0)));
        assert!(chip_text_index(&commands).is_none(), "no chip is drawn");
        assert_eq!(app.hover_chips.borrow().len(), 0);
    }

    #[test]
    fn a_hovered_chip_is_drawn_once_and_not_into_the_next_frame() {
        let app = AppContext::default();
        let mut root = covered_chip();

        let commands = paint_frame(&mut root, &app, Some(vec2f(20.0, 16.0)));
        assert_eq!(chip_text_index(&commands), Some(commands.len() - 1));
        assert!(
            chip_text_index(&commands[..commands.len() - 1]).is_none(),
            "the chip is drawn exactly once"
        );
        assert!(
            app.hover_chips.borrow().is_empty(),
            "the layer empties the registry"
        );

        let commands = paint_frame(&mut root, &app, None);
        assert!(
            chip_text_index(&commands).is_none(),
            "the next frame draws no leftover chip"
        );
    }
}
