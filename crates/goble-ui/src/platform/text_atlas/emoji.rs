//! Colour emoji: the glyphs a monochrome coverage atlas cannot carry.
//!
//! The text atlas stores one coverage byte per texel and the text shader tints
//! it with the run's colour, which is what keeps prose cheap. A colour emoji is
//! a small RGBA bitmap instead, so it is rasterized on its own here and drawn
//! over its run as an image quad. The run's layout still comes from fontdue: the
//! emoji keeps the cell the fallback face gave it, and only its ink is replaced.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// One colour cluster rasterized to a straight-alpha RGBA8 bitmap.
pub(super) struct EmojiBitmap {
    /// `width * height * 4` bytes, straight alpha (the image pipeline blends
    /// with `SrcAlpha`/`OneMinusSrcAlpha`).
    pub rgba: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
    /// How far below the bitmap's top the text baseline sits.
    pub ascent: f32,
}

/// How many emoji bitmaps are kept before the cache is emptied. A transcript
/// shows a handful of distinct emoji; the bound only exists so a terminal that
/// printed every emoji in the Unicode table would not grow it forever.
const MAX_EMOJI: usize = 512;

thread_local! {
    static CACHE: RefCell<HashMap<(String, u32), Option<Arc<EmojiBitmap>>>> =
        RefCell::new(HashMap::new());
}

/// Whether a grapheme cluster is an emoji, i.e. one the system's colour emoji
/// face should draw rather than the monochrome fallback's outline.
///
/// A cluster qualifies when it holds an emoji-presentation code point or a
/// variation selector requesting emoji presentation (`⚠️`, `©️` and keycaps).
/// Text-presentation symbols (`⚠` alone) keep the monochrome glyph they had.
pub(super) fn is_emoji_cluster(cluster: &str) -> bool {
    let mut chars = cluster.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first == '\u{fe0f}' {
        return false;
    }
    let mut emoji = is_emoji_base(first);
    let mut emoji_selector = false;
    for ch in chars {
        emoji |= is_emoji_base(ch);
        emoji_selector |= ch == '\u{fe0f}';
    }
    emoji || emoji_selector
}

/// Code points that default to emoji presentation. Text-default symbols that
/// only become emoji with a variation selector are deliberately absent: the UI
/// draws several of them as monochrome marks.
fn is_emoji_base(ch: char) -> bool {
    matches!(
        ch,
        '\u{1F000}'..='\u{1FAFF}'
            | '\u{231A}'..='\u{231B}'
            | '\u{23E9}'..='\u{23EC}'
            | '\u{23F0}'
            | '\u{23F3}'
            | '\u{25FD}'..='\u{25FE}'
            | '\u{2614}'..='\u{2615}'
            | '\u{2648}'..='\u{2653}'
            | '\u{267F}'
            | '\u{2693}'
            | '\u{26A1}'
            | '\u{26AA}'..='\u{26AB}'
            | '\u{26BD}'..='\u{26BE}'
            | '\u{26C4}'..='\u{26C5}'
            | '\u{26CE}'
            | '\u{26D4}'
            | '\u{26EA}'
            | '\u{26F2}'..='\u{26F3}'
            | '\u{26F5}'
            | '\u{26FA}'
            | '\u{26FD}'
            | '\u{2705}'
            | '\u{270A}'..='\u{270B}'
            | '\u{2728}'
            | '\u{274C}'
            | '\u{274E}'
            | '\u{2753}'..='\u{2755}'
            | '\u{2757}'
            | '\u{2795}'..='\u{2797}'
            | '\u{27B0}'
            | '\u{27BF}'
            | '\u{2B1B}'..='\u{2B1C}'
            | '\u{2B50}'
            | '\u{2B55}'
    )
}

/// Rasterize `cluster` through the system's colour emoji face at `px` physical
/// pixels. `None` on a platform (or system) without a colour emoji face: the
/// caller then keeps the monochrome fallback glyph.
pub(super) fn rasterize(cluster: &str, px: f32) -> Option<Arc<EmojiBitmap>> {
    if px <= 0.0 || !px.is_finite() {
        return None;
    }
    let key = (cluster.to_string(), px.to_bits());
    if let Some(hit) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return hit;
    }
    let bitmap = rasterize_uncached(cluster, px).map(Arc::new);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_EMOJI {
            cache.clear();
        }
        cache.insert(key, bitmap.clone());
    });
    bitmap
}

#[cfg(target_os = "macos")]
fn rasterize_uncached(cluster: &str, px: f32) -> Option<EmojiBitmap> {
    use core_foundation::attributed_string::CFMutableAttributedString;
    use core_foundation::base::{CFRange, TCFType};
    use core_foundation::string::CFString;
    use core_graphics::base::kCGImageAlphaPremultipliedLast;
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use core_graphics::geometry::{CGPoint, CGRect, CGSize};
    use core_text::line::CTLine;
    use core_text::string_attributes::kCTFontAttributeName;

    let font = core_text::font::new_from_name("Apple Color Emoji", px as f64).ok()?;
    let string = CFString::new(cluster);
    let mut attributed = CFMutableAttributedString::new();
    attributed.replace_str(&string, CFRange { location: 0, length: 0 });
    attributed.set_attribute(
        CFRange {
            location: 0,
            length: string.char_len(),
        },
        unsafe { kCTFontAttributeName },
        &font,
    );
    let line = CTLine::new_with_attributed_string(attributed.as_concrete_TypeRef());
    let metrics = line.get_typographic_bounds();
    if metrics.width <= 0.0 || metrics.ascent + metrics.descent <= 0.0 {
        return None;
    }

    let width = (metrics.width.ceil() as usize).max(1);
    let height = ((metrics.ascent + metrics.descent).ceil() as usize).max(1);
    let stride = width * 4;
    let mut pixels = vec![0u8; stride * height];
    let color_space = CGColorSpace::create_device_rgb();
    let context = CGContext::create_bitmap_context(
        Some(pixels.as_mut_ptr() as *mut _),
        width,
        height,
        8,
        stride,
        &color_space,
        kCGImageAlphaPremultipliedLast,
    );
    // The context starts opaque black; a transparent field is what lets the
    // run's background show through the emoji's own transparent corners.
    context.set_rgb_fill_color(0.0, 0.0, 0.0, 0.0);
    context.fill_rect(CGRect::new(
        &CGPoint::new(0.0, 0.0),
        &CGSize::new(width as f64, height as f64),
    ));
    context.set_allows_font_smoothing(true);
    context.set_should_smooth_fonts(true);
    context.set_should_antialias(true);
    // Core Graphics draws from the bottom left; put the baseline `descent`
    // above it so the whole ink box fits the bitmap.
    context.translate(0.0, metrics.descent);
    context.set_text_position(0.0, 0.0);
    line.draw(&context);

    // Core Graphics writes premultiplied RGBA; the image pipeline blends
    // straight alpha.
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = pixel[3] as u32;
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
        } else if alpha < 255 {
            pixel[0] = ((pixel[0] as u32 * 255) / alpha).min(255) as u8;
            pixel[1] = ((pixel[1] as u32 * 255) / alpha).min(255) as u8;
            pixel[2] = ((pixel[2] as u32 * 255) / alpha).min(255) as u8;
        }
    }

    Some(EmojiBitmap {
        rgba: pixels.into(),
        width: width as u32,
        height: height as u32,
        ascent: metrics.ascent as f32,
    })
}

#[cfg(not(target_os = "macos"))]
fn rasterize_uncached(_cluster: &str, _px: f32) -> Option<EmojiBitmap> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_clusters_are_recognised() {
        assert!(is_emoji_cluster("👋"));
        assert!(is_emoji_cluster("⚠️"));
        assert!(!is_emoji_cluster("⚠"));
        assert!(!is_emoji_cluster("a"));
        assert!(!is_emoji_cluster("◆"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_face_draws_a_coloured_wave() {
        let bitmap = rasterize("👋", 32.0).expect("Apple Color Emoji rasterizes");
        assert!(bitmap.width > 0 && bitmap.height > 0);
        let coloured = bitmap.rgba.chunks_exact(4).any(|pixel| {
            let spread = (pixel[0] as i32 - pixel[1] as i32).abs()
                + (pixel[1] as i32 - pixel[2] as i32).abs();
            pixel[3] > 0 && spread > 20
        });
        assert!(
            coloured,
            "the emoji carries its own colour instead of a grey mask"
        );
    }
}
