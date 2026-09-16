//! The glyph atlas: rasterized text runs cached in one texture, plus the font
//! metrics the elements lay out against.
//!
//! One module per surface: the atlas itself ([`atlas`]), the bundled fonts it
//! draws with ([`fonts`]) and the measurement cache ([`cache`]). The public
//! items stay at the module root so call sites keep reading
//! `platform::text_atlas::…`.

mod atlas;
mod cache;
mod fonts;

#[cfg(test)]
mod tests;

pub use atlas::{AtlasEntry, TextAtlas};
pub use cache::{invalidate_text_measure_cache, measure_cache_stats, MeasureCacheStats};
pub use fonts::{
    advance_width, font_covers, measure_text, measure_text_family, mono_advance, FontWeight,
    TextMetrics,
};
