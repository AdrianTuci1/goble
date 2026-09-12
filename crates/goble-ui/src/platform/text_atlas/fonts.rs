use std::collections::HashMap;
use std::sync::OnceLock;

use crate::theme::FontFamily;

use super::atlas::AtlasEntry;

/// Bundled Roboto font weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Bold,
    SemiBold,
}

/// Real text metrics returned by [`measure_text`].
#[derive(Clone, Copy, Debug)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

/// Measures a single-line or wrapped text block using the bundled Roboto fonts.
///
/// This is the source of truth for layout in elements such as [`crate::elements::Text`].
/// If the bundled fonts cannot be loaded it returns a conservative heuristic so that
/// layout never panics.
pub fn measure_text(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
) -> crate::geometry::Vector2F {
    measure_text_family(
        text,
        font_size,
        line_height,
        max_width,
        weight,
        FontFamily::System,
        false,
    )
}

/// Like [`measure_text`] but using an explicit font family (e.g. mono for
/// terminals) and an oblique/italic face.
pub fn measure_text_family(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> crate::geometry::Vector2F {
    let Some(font_set) = font_set() else {
        return estimate_text_size(text, font_size, line_height, max_width);
    };
    let font = font_set.select(weight, family, italic);
    let fonts = &[font.clone()];
    let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
    layout.reset(&fontdue::layout::LayoutSettings {
        max_width: if max_width.is_finite() && max_width > 0.0 {
            Some(max_width)
        } else {
            None
        },
        max_height: None,
        line_height,
        ..Default::default()
    });
    layout.append(fonts, &fontdue::layout::TextStyle::new(text, font_size, 0));

    if layout.glyphs().is_empty() {
        return crate::geometry::vec2f(0.0, font_size * line_height);
    }

    let width = layout
        .glyphs()
        .iter()
        .map(|g| g.x + g.width as f32)
        .fold(0.0, f32::max)
        .ceil();
    let height = layout.height().ceil().max(font_size * line_height);
    crate::geometry::vec2f(width, height)
}

/// The advance width of one character in the bundled monospace font.
///
/// A cell grid has to know its column pitch before it can place anything, and
/// the pitch is a property of the font rather than of the string being drawn,
/// so it is read from the font's own metrics and cached instead of being
/// measured per frame. Terminals otherwise fall back to a hand-tuned ratio,
/// which drifts from the real glyphs and makes a right-hand border wander.
pub fn mono_advance(font_size: f32, weight: FontWeight) -> f32 {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<(u32, u32), f32>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let key = (font_size.to_bits(), weight as u32);
    if let Ok(cache) = cache.lock() {
        if let Some(advance) = cache.get(&key) {
            return *advance;
        }
    }

    let Some(font_set) = font_set() else {
        return font_size * 0.6;
    };
    let font = font_set.select(weight, FontFamily::Mono, false);
    let advance = font.metrics('M', font_size).advance_width;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, advance);
    }
    advance
}

fn estimate_text_size(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
) -> crate::geometry::Vector2F {
    const APPROX_CHAR_WIDTH_RATIO: f32 = 0.55;
    if text.is_empty() {
        return crate::geometry::vec2f(0.0, font_size * line_height);
    }
    let char_width = font_size * APPROX_CHAR_WIDTH_RATIO;
    let full_width = text.chars().count() as f32 * char_width;
    if full_width <= max_width || max_width.is_infinite() || max_width <= 0.0 {
        return crate::geometry::vec2f(full_width, font_size * line_height);
    }
    let chars_per_line = (max_width / char_width).max(1.0) as usize;
    let total_chars = text.chars().count();
    let raw_lines = (total_chars + chars_per_line - 1) / chars_per_line.max(1);
    let line_count = raw_lines.max(1);
    let width = (chars_per_line as f32 * char_width).min(full_width);
    crate::geometry::vec2f(width, font_size * line_height * line_count as f32)
}

pub(super) struct FontSet {
    regular: fontdue::Font,
    medium: fontdue::Font,
    semibold: fontdue::Font,
    bold: fontdue::Font,
    italic: fontdue::Font,
    bold_italic: fontdue::Font,
    mono: fontdue::Font,
    mono_bold: fontdue::Font,
    mono_italic: fontdue::Font,
    mono_bold_italic: fontdue::Font,
}

impl FontSet {
    pub(super) fn select(
        &self,
        weight: FontWeight,
        family: FontFamily,
        italic: bool,
    ) -> &fontdue::Font {
        match (family, italic) {
            (FontFamily::Mono, false) => match weight {
                FontWeight::Bold | FontWeight::SemiBold => &self.mono_bold,
                FontWeight::Regular | FontWeight::Medium => &self.mono,
            },
            (FontFamily::Mono, true) => match weight {
                FontWeight::Bold | FontWeight::SemiBold => &self.mono_bold_italic,
                FontWeight::Regular | FontWeight::Medium => &self.mono_italic,
            },
            (FontFamily::System | FontFamily::Serif, false) => match weight {
                FontWeight::Regular => &self.regular,
                FontWeight::Medium => &self.medium,
                FontWeight::SemiBold => &self.semibold,
                FontWeight::Bold => &self.bold,
            },
            (FontFamily::System | FontFamily::Serif, true) => match weight {
                FontWeight::Bold | FontWeight::SemiBold => &self.bold_italic,
                FontWeight::Regular | FontWeight::Medium => &self.italic,
            },
        }
    }
}

pub(super) fn font_set() -> Option<&'static FontSet> {
    static FONTS: OnceLock<Option<FontSet>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let regular = load_bundled_font(FontWeight::Regular, FontFamily::System, false)?;
            let medium = load_bundled_font(FontWeight::Medium, FontFamily::System, false)
                .unwrap_or_else(|| regular.clone());
            let bold = load_bundled_font(FontWeight::Bold, FontFamily::System, false)
                .unwrap_or_else(|| regular.clone());
            let semibold = load_bundled_font(FontWeight::SemiBold, FontFamily::System, false)
                .unwrap_or_else(|| bold.clone());
            let italic = load_bundled_font(FontWeight::Regular, FontFamily::System, true)
                .unwrap_or_else(|| regular.clone());
            let bold_italic = load_bundled_font(FontWeight::Bold, FontFamily::System, true)
                .unwrap_or_else(|| bold.clone());
            let mono = load_bundled_font(FontWeight::Regular, FontFamily::Mono, false)
                .unwrap_or_else(|| regular.clone());
            let mono_bold = load_bundled_font(FontWeight::Bold, FontFamily::Mono, false)
                .unwrap_or_else(|| mono.clone());
            let mono_italic = load_bundled_font(FontWeight::Regular, FontFamily::Mono, true)
                .unwrap_or_else(|| mono.clone());
            let mono_bold_italic = load_bundled_font(FontWeight::Bold, FontFamily::Mono, true)
                .unwrap_or_else(|| mono_bold.clone());
            Some(FontSet {
                regular,
                medium,
                semibold,
                bold,
                italic,
                bold_italic,
                mono,
                mono_bold,
                mono_italic,
                mono_bold_italic,
            })
        })
        .as_ref()
}

fn load_bundled_font(
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> Option<fontdue::Font> {
    let bytes: &[u8] = match (family, italic, weight) {
        (FontFamily::Mono, false, FontWeight::Bold | FontWeight::SemiBold) => {
            include_bytes!("../../../assets/fonts/hack/Hack-Bold.ttf")
        }
        (FontFamily::Mono, false, _) => include_bytes!("../../../assets/fonts/hack/Hack-Regular.ttf"),
        (FontFamily::Mono, true, FontWeight::Bold | FontWeight::SemiBold) => {
            include_bytes!("../../../assets/fonts/hack/Hack-BoldItalic.ttf")
        }
        (FontFamily::Mono, true, _) => include_bytes!("../../../assets/fonts/hack/Hack-Italic.ttf"),
        (_, false, FontWeight::Regular) => {
            include_bytes!("../../../assets/fonts/roboto/Roboto-Regular.ttf")
        }
        (_, false, FontWeight::Medium) => {
            include_bytes!("../../../assets/fonts/roboto/Roboto-Medium.ttf")
        }
        (_, false, FontWeight::Bold) => include_bytes!("../../../assets/fonts/roboto/Roboto-Bold.ttf"),
        (_, false, FontWeight::SemiBold) => {
            include_bytes!("../../../assets/fonts/roboto/RobotoFlex-Semibold.ttf")
        }
        (_, true, FontWeight::Bold | FontWeight::SemiBold) => {
            include_bytes!("../../../assets/fonts/roboto/Roboto-BoldItalic.ttf")
        }
        (_, true, _) => include_bytes!("../../../assets/fonts/roboto/Roboto-Italic.ttf"),
    };
    fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
}

pub(super) fn rasterize_text(
    text: &str,
    font_size: u32,
    weight: FontWeight,
    mono: bool,
    italic: bool,
    max_width: f32,
    line_height: f32,
) -> Option<(AtlasEntry, Vec<u8>, u32, u32)> {
    let font_set = font_set()?;
    let family = if mono {
        FontFamily::Mono
    } else {
        FontFamily::System
    };
    let font = font_set.select(weight, family, italic);
    let fonts = &[font.clone()];
    let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
    layout.reset(&fontdue::layout::LayoutSettings {
        max_width: if max_width.is_finite() && max_width > 0.0 {
            Some(max_width)
        } else {
            None
        },
        max_height: None,
        line_height,
        ..Default::default()
    });
    layout.append(
        fonts,
        &fontdue::layout::TextStyle::new(text, font_size as f32, 0),
    );

    let glyphs = layout.glyphs();
    // The quad spans the block's line box, so the drawn ink lands exactly where
    // `measure_text_family` sized the element (ascents, descents and line gaps
    // are all accounted for by `layout.height`).
    let line_box_height = layout
        .height()
        .ceil()
        .max(font_size as f32 * line_height)
        .ceil();

    if glyphs.is_empty() {
        let height = line_box_height.max(1.0) as u32;
        let entry = AtlasEntry {
            uv_origin: [0.0; 2],
            uv_size: [0.0; 2],
            size: [1.0, height as f32],
            offset: [0.0; 2],
        };
        return Some((entry, vec![0u8; height as usize], 1, height));
    }

    // Fontdue already bakes a glyph's bearings into its position (`glyph.x` is
    // the bitmap's left edge, `glyph.y` its top), so the bitmap is copied
    // straight to its layout coordinates instead of adding `xmin`/`ymin` again.
    // Measuring both `max_x` and `max_y` lets the quad shrink to the ink when
    // that is taller/narrower than the line box, so nothing gets clipped.
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for glyph in glyphs {
        let (metrics, _) = font.rasterize_config(glyph.key);
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        min_x = min_x.min(glyph.x);
        min_y = min_y.min(glyph.y);
        max_x = max_x.max(glyph.x + metrics.width as f32);
        max_y = max_y.max(glyph.y + metrics.height as f32);
    }

    // Anchor the bitmap at the line-box origin so the quad aligns with the
    // element box; glyphs that overhang above/left still fit instead of being
    // clipped.
    let left = min_x.min(0.0).floor();
    let top = min_y.min(0.0).floor();
    let width = ((max_x - left).ceil() as u32).max(1);
    let height = ((line_box_height.max(max_y - top).ceil()).max(1.0)) as u32;

    let mut atlas = vec![0u8; (width * height) as usize];
    for glyph in glyphs {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        let x_offset = (glyph.x - left).floor() as i32;
        let y_offset = (glyph.y - top).floor() as i32;
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let src = row * metrics.width + col;
                let dst_x = x_offset + col as i32;
                let dst_y = y_offset + row as i32;
                if dst_x < 0 || dst_y < 0 || dst_x >= width as i32 || dst_y >= height as i32 {
                    continue;
                }
                let dst = (dst_y * width as i32 + dst_x) as usize;
                atlas[dst] = atlas[dst].saturating_add(bitmap[src]);
            }
        }
    }

    let entry = AtlasEntry {
        uv_origin: [0.0; 2],
        uv_size: [0.0; 2],
        size: [width as f32, height as f32],
        offset: [-left, -top],
    };

    Some((entry, atlas, width, height))
}
