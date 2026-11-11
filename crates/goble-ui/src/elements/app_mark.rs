//! The Goble mark in full colour.
//!
//! The icon atlas keeps one alpha mask per glyph and paints it in a single
//! tint. That turns the app mark — a dark rounded square carrying a gradient
//! blob — into a flat block, so the sidebar loses the artwork it leads with.
//! This element draws the bundled RGBA mark instead, at the size a card gives
//! it.

use std::sync::{Arc, OnceLock};

use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{rectf, vec2f, Vector2F};

/// The artwork, baked from the app icon so the mark and the dock agree.
const MARK_PNG: &[u8] = include_bytes!("../../assets/icons/harness-goble.png");

/// The same mark framed out: the octopus alone, with no rounded-square
/// background. A bar that names the native harness carries the octopus only —
/// the square is the app's frame, and a chip is not the app.
const OCTOPUS_PNG: &[u8] = include_bytes!("../../assets/icons/goble-octopus.png");

/// The texture's source key. One mark, one key; the renderer caches the
/// uploaded texture under it and leaves it alone across frames.
const MARK_SOURCE: &str = "harness-goble-mark";
const OCTOPUS_SOURCE: &str = "harness-goble-octopus";

struct Pixels {
    width: u32,
    height: u32,
    data: Arc<[u8]>,
}

/// Decode one bundled mark once for the life of the process.
fn decode(bytes: &'static [u8]) -> Pixels {
    let image = image::load_from_memory(bytes)
        .expect("the bundled Goble mark decodes")
        .to_rgba8();
    let (width, height) = image.dimensions();
    Pixels {
        width,
        height,
        data: Arc::from(image.into_raw()),
    }
}

/// The decoded full mark, the artwork the sidebar and the dock share.
fn pixels() -> &'static Pixels {
    static PIXELS: OnceLock<Pixels> = OnceLock::new();
    PIXELS.get_or_init(|| decode(MARK_PNG))
}

/// The decoded octopus, the mark without the app's square frame.
fn octopus_pixels() -> &'static Pixels {
    static PIXELS: OnceLock<Pixels> = OnceLock::new();
    PIXELS.get_or_init(|| decode(OCTOPUS_PNG))
}

/// The Goble app mark, drawn as the full-colour artwork.
pub struct AppMark {
    size: f32,
    /// Whether the mark is drawn with the app's square frame or as the octopus
    /// alone.
    frameless: bool,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AppMark {
    pub fn new(size: f32) -> Self {
        Self {
            size,
            frameless: false,
            layout_size: None,
            origin: None,
        }
    }

    /// The octopus alone, without the rounded-square background: the form a
    /// bar uses when it names the native harness beside its title.
    pub fn octopus(size: f32) -> Self {
        Self {
            size,
            frameless: true,
            layout_size: None,
            origin: None,
        }
    }

    fn artwork(&self) -> (&'static str, &'static Pixels) {
        if self.frameless {
            (OCTOPUS_SOURCE, octopus_pixels())
        } else {
            (MARK_SOURCE, pixels())
        }
    }
}

impl Element for AppMark {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.size, self.size);
        self.layout_size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let (source, pixels) = self.artwork();
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.draw_image(
                rectf(origin.x, origin.y, self.size, self.size),
                source,
                pixels.width,
                pixels.height,
                0,
                Arc::clone(&pixels.data),
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.layout_size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::RenderCommand;

    #[test]
    fn the_mark_paints_the_full_colour_artwork() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(AppMark::new(16.0));
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let (rect, width, height) = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawImage {
                    rect,
                    source,
                    width,
                    height,
                    ..
                } if source == MARK_SOURCE => Some((*rect, *width, *height)),
                _ => None,
            })
            .expect("the mark draws its artwork");
        assert_eq!((rect.width(), rect.height()), (16.0, 16.0));
        assert_eq!(
            (width, height),
            (256, 256),
            "the bundled mark is 256 px square"
        );
    }
}
