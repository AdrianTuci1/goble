//! The glyph atlas: rasterized text runs cached in one texture, plus the font
//! metrics the elements lay out against.
//!
//! One module per surface: the atlas itself ([`atlas`]) and the bundled fonts
//! it draws with ([`fonts`]). The public items stay at the module root so call
//! sites keep reading `platform::text_atlas::…`.

mod atlas;
mod fonts;

#[cfg(test)]
mod tests;

pub use atlas::{AtlasEntry, TextAtlas};
pub use fonts::{measure_text, measure_text_family, mono_advance, FontWeight, TextMetrics};
