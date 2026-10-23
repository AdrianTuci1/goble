use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::theme::FontFamily;

use super::atlas::AtlasEntry;
use super::cache;

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
///
/// Memoized by every number the answer depends on: the tree is rebuilt every
/// frame, so a transcript asks for the same few hundred runs over and over.
pub fn measure_text_family(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> crate::geometry::Vector2F {
    let key = cache::SizeKey::new(
        text, font_size, line_height, max_width, weight, family, italic,
    );
    if let Some(size) = cache::lookup_size(&key) {
        return size;
    }
    let size = measure_uncached(text, font_size, line_height, max_width, weight, family, italic);
    cache::store_size(key, size);
    size
}

/// One visual line a run wraps into, as the renderer draws it.
///
/// `text` is the exact source text the line shows, so a selection can copy
/// what the user sees rather than re-breaking the paragraph with different
/// rules. `top` is the line's top relative to the run's own origin and
/// `height` the row it occupies.
#[derive(Clone, Debug, PartialEq)]
pub struct WrappedLine {
    pub text: String,
    pub top: f32,
    pub height: f32,
}

/// The lines `text` wraps into at `max_width`, with the geometry each line is
/// drawn at.
///
/// The breaks are fontdue's own — the pass [`measure_text_family`] sizes the
/// box and the renderer draws it with — so a line here starts and ends on the
/// same characters the user sees on that row. Memoized like the measurements:
/// a transcript asks for every visible run's lines on every frame while a
/// selection is being dragged.
pub fn wrapped_lines(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> Vec<WrappedLine> {
    let key = cache::SizeKey::new(
        text, font_size, line_height, max_width, weight, family, italic,
    );
    if let Some(lines) = cache::lookup_lines(&key) {
        return lines;
    }
    let lines = wrapped_lines_uncached(text, font_size, line_height, max_width, weight, family, italic);
    cache::store_lines(key, lines.clone());
    lines
}

fn wrapped_lines_uncached(
    text: &str,
    font_size: f32,
    line_height: f32,
    max_width: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> Vec<WrappedLine> {
    let pitch = font_size * line_height;
    let Some(font_set) = font_set() else {
        return text
            .split('\n')
            .enumerate()
            .map(|(index, line)| WrappedLine {
                text: line.to_string(),
                top: index as f32 * pitch,
                height: pitch,
            })
            .collect();
    };
    let font = font_set.select(weight, family, italic);
    let chain = font_chain(font, text);
    let (layout, runs) = layout_text_with_runs(text, font_size, max_width, line_height, &chain);
    let glyphs = layout.glyphs();
    let Some(lines) = layout.lines() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let start = line.glyph_start.min(glyphs.len());
        let end = line.glyph_end.saturating_add(1).min(glyphs.len());
        let mut line_text = String::new();
        // The end of the source text the previous glyph on this line covered.
        // The gap to the next glyph is the whitespace fontdue placed without a
        // glyph of its own (and the wrap break it dropped), which belongs to
        // the line's text.
        let mut covered = 0usize;
        for (index, glyph) in glyphs.iter().enumerate().take(end).skip(start) {
            let offset = run_base(index, &runs) + glyph.byte_offset;
            if covered > 0 && offset > covered && offset <= text.len() {
                line_text.extend(text[covered..offset].chars().filter(|ch| *ch != '\n'));
            }
            if let Some(ch) = text.get(offset..).and_then(|rest| rest.chars().next()) {
                line_text.push(ch);
                covered = offset + ch.len_utf8();
            }
        }
        out.push(WrappedLine {
            text: line_text.trim_end().to_string(),
            top: (line.baseline_y - line.max_ascent).max(0.0),
            height: (line.max_ascent - line.min_descent + line.max_line_gap)
                .max(font_size)
                * line_height,
        });
    }
    out
}

/// [`measure_text_family`] without the cache: the fontdue pass itself.
pub(super) fn measure_uncached(
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
    let chain = font_chain(font, text);
    let layout = layout_text(text, font_size, max_width, line_height, &chain);

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

/// The advance width of a single-line run: how far past its own origin the next
/// character would start.
///
/// [`measure_text_family`] reports the ink extent, which is what a box around a
/// run needs, but a flow that packs runs onto a line has to budget the
/// advances: fontdue breaks a line when `current_pos + ceil(advance)` exceeds
/// the width it was given, and the ink extent is a side bearing short of that.
/// Packing by the ink extent therefore leaves runs that the renderer breaks
/// again inside the run, which moves a word down a line.
///
/// The sum is taken the way fontdue's own layout takes it, so `text` fits a
/// `max_width` of exactly this value and no less.
///
/// Memoized like [`measure_text_family`]: a paragraph asks for the advance of
/// every word it holds, on every frame.
pub fn advance_width(
    text: &str,
    font_size: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> f32 {
    let key = cache::AdvanceKey::new(text, font_size, weight, family, italic);
    if let Some(advance) = cache::lookup_advance(&key) {
        return advance;
    }
    let advance = advance_width_uncached(text, font_size, weight, family, italic);
    cache::store_advance(key, advance);
    advance
}

/// [`advance_width`] without the cache: the fontdue pass itself.
pub(super) fn advance_width_uncached(
    text: &str,
    font_size: f32,
    weight: FontWeight,
    family: FontFamily,
    italic: bool,
) -> f32 {
    let Some(font_set) = font_set() else {
        return text.chars().count() as f32 * font_size * 0.6;
    };
    let font = font_set.select(weight, family, italic);
    let chain = font_chain(font, text);
    let layout = layout_text(text, font_size, f32::INFINITY, 1.0, &chain);
    layout
        .glyphs()
        .iter()
        .map(|glyph| {
            // A control character is laid out with no advance by fontdue; every
            // other glyph advances by its own, rounded up to a whole pixel.
            if glyph.char_data.is_control() {
                0.0
            } else {
                chain[glyph.font_index]
                    .metrics_indexed(glyph.key.glyph_index, glyph.key.px)
                    .advance_width
                    .ceil()
                    .max(0.0)
            }
        })
        .sum()
}

/// The fonts one run lays out through, in order: the requested face first, then
/// the fallbacks when the run holds a character that face does not draw.
///
/// fontdue lays a run out in one face at a time, so the fallbacks are only
/// loaded (and only consulted) when the primary face cannot draw something.
/// Without them a character outside the bundled faces — an arrow, a box-drawing
/// cell, a CJK glyph — rasterizes as an empty `.notdef` box.
fn font_chain<'a>(primary: &'a fontdue::Font, text: &str) -> Vec<&'a fontdue::Font> {
    let mut chain = Vec::with_capacity(4);
    chain.push(primary);
    if text
        .chars()
        .any(|ch| !ch.is_whitespace() && !primary.has_glyph(ch))
    {
        chain.extend(fallback_fonts());
    }
    chain
}

/// One appended stretch of the source text, and the glyphs it produced.
///
/// A run is appended per font the fallback chain switches to, and fontdue
/// reports each glyph's byte offset relative to its own run. The base is what
/// turns it back into an offset into the caller's whole string.
struct GlyphRun {
    first_glyph: usize,
    end_glyph: usize,
    byte_base: usize,
}

/// Layout `text` through `fonts`, splitting the run at every character the
/// current face cannot draw and continuing with the first face that can.
fn layout_text(
    text: &str,
    font_size: f32,
    max_width: f32,
    line_height: f32,
    fonts: &[&fontdue::Font],
) -> fontdue::layout::Layout {
    layout_text_with_runs(text, font_size, max_width, line_height, fonts).0
}

fn layout_text_with_runs(
    text: &str,
    font_size: f32,
    max_width: f32,
    line_height: f32,
    fonts: &[&fontdue::Font],
) -> (fontdue::layout::Layout, Vec<GlyphRun>) {
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
    let runs = append_with_fallback(&mut layout, fonts, text, font_size);
    (layout, runs)
}

fn append_with_fallback(
    layout: &mut fontdue::layout::Layout,
    fonts: &[&fontdue::Font],
    text: &str,
    font_size: f32,
) -> Vec<GlyphRun> {
    let mut runs = Vec::new();
    let Some(first) = text.chars().next() else {
        return runs;
    };
    let face_for = |ch: char| -> usize {
        if ch.is_whitespace() || ch.is_control() {
            return 0;
        }
        fonts
            .iter()
            .position(|font| draws_glyph(font, ch))
            .unwrap_or(0)
    };
    let mut start = 0;
    let mut current = face_for(first);
    for (offset, ch) in text.char_indices().skip(1) {
        let face = face_for(ch);
        if face != current {
            append_run(
                layout,
                fonts,
                &text[start..offset],
                start,
                font_size,
                current,
                &mut runs,
            );
            start = offset;
            current = face;
        }
    }
    append_run(
        layout,
        fonts,
        &text[start..],
        start,
        font_size,
        current,
        &mut runs,
    );
    runs
}

fn append_run(
    layout: &mut fontdue::layout::Layout,
    fonts: &[&fontdue::Font],
    segment: &str,
    byte_base: usize,
    font_size: f32,
    face: usize,
    runs: &mut Vec<GlyphRun>,
) {
    let first_glyph = layout.glyphs().len();
    layout.append(
        fonts,
        &fontdue::layout::TextStyle::new(segment, font_size, face),
    );
    runs.push(GlyphRun {
        first_glyph,
        end_glyph: layout.glyphs().len(),
        byte_base,
    });
}

/// The byte base of the run that produced glyph `index`.
fn run_base(index: usize, runs: &[GlyphRun]) -> usize {
    runs.iter()
        .find(|run| index >= run.first_glyph && index < run.end_glyph)
        .map_or(0, |run| run.byte_base)
}

/// Whether a face can actually draw `ch`.
///
/// `has_glyph` only says the face's character map names a glyph. A colour or
/// bitmap-only face (Apple Color Emoji, say) names one and still rasterizes
/// nothing, so the chain would hand it a character it cannot ink. Metrics are
/// computed from the outline, so a zero-sized glyph is skipped and the next
/// face is tried.
fn draws_glyph(font: &fontdue::Font, ch: char) -> bool {
    font.has_glyph(ch) && {
        let metrics = font.metrics(ch, 16.0);
        metrics.width > 0 && metrics.height > 0
    }
}

/// Faces tried after the bundled primary, in coverage order: the system's
/// fallbacks first, then the bundled monochrome emoji face.
///
/// They are read once, on first use, so a run that stays inside Roboto or Hack
/// pays nothing for them. The emoji face stays bundled rather than borrowed
/// from the system because macOS's only emoji font is a colour bitmap font,
/// which a monochrome coverage atlas cannot rasterize.
fn fallback_fonts() -> &'static [fontdue::Font] {
    static FALLBACKS: OnceLock<Vec<fontdue::Font>> = OnceLock::new();
    FALLBACKS.get_or_init(|| {
        let mut fonts: Vec<fontdue::Font> = fallback_paths()
            .into_iter()
            .filter_map(|path| std::fs::read(path).ok())
            .filter_map(|bytes| {
                fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
            })
            .collect();
        if let Ok(emoji) = fontdue::Font::from_bytes(
            include_bytes!("../../../assets/fonts/noto-emoji/NotoEmoji-Regular.ttf").as_slice(),
            fontdue::FontSettings::default(),
        ) {
            fonts.push(emoji);
        }
        fonts
    })
}

/// The system faces tried after the bundled one, in coverage order. A missing
/// file is skipped, so the same list is safe on a machine without the optional
/// faces.
fn fallback_paths() -> Vec<&'static str> {
    #[cfg(target_os = "macos")]
    {
        vec![
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Apple Symbols.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ]
    }
    #[cfg(target_os = "linux")]
    {
        vec![
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ]
    }
    #[cfg(target_os = "windows")]
    {
        vec![
            "C:\\Windows\\Fonts\\consola.ttf",
            "C:\\Windows\\Fonts\\seguisym.ttf",
            "C:\\Windows\\Fonts\\arialuni.ttf",
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\malgun.ttf",
            "C:\\Windows\\Fonts\\Nirmala.ttf",
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Whether the bundled face that draws `text` covers every character in it.
///
/// The fonts are bundled (Roboto for prose, Hack for the terminal), so a
/// character outside their coverage — the Command or Shift key symbol, say —
/// would rasterize as the font's `.notdef` box instead of the glyph the UI
/// means. A caller that has an icon for such a character asks here first and
/// draws the icon when the answer is `false`.
pub fn font_covers(text: &str, family: FontFamily, weight: FontWeight) -> bool {
    let Some(font_set) = font_set() else {
        return true;
    };
    let font = font_set.select(weight, family, false);
    text.chars().all(|ch| ch.is_whitespace() || font.has_glyph(ch))
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

/// One glyph's coverage bitmap and its metrics, cached by the font, glyph and
/// size it was rasterized for.
struct GlyphBitmap {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

/// How many glyphs the cache keeps before it is emptied. A code view crosses a
/// few hundred distinct glyphs at one size; the bound is there for a window
/// that has been zoomed through many scales, not for ordinary use.
const MAX_GLYPHS: usize = 8192;

thread_local! {
    /// Rasterized glyphs, keyed by font, glyph and size.
    ///
    /// A run is what the atlas stores, but a line of code is made of glyphs the
    /// lines before it already rasterized: caching at the glyph is what makes a
    /// line scrolled into view cost a composition of cached bitmaps instead of
    /// an outline parse and rasterization for every character. Warp-new's
    /// renderer keeps a glyph cache for the same reason, and this is the piece
    /// of its approach that fits the run atlas without changing the renderer.
    static GLYPH_CACHE: RefCell<HashMap<fontdue::layout::GlyphRasterConfig, Rc<GlyphBitmap>>> =
        RefCell::new(HashMap::new());
}

/// The bitmap for `config`, rasterized once per font/glyph/size.
fn glyph_bitmap(
    font: &fontdue::Font,
    config: fontdue::layout::GlyphRasterConfig,
) -> Rc<GlyphBitmap> {
    GLYPH_CACHE.with(|cache| {
        if let Some(glyph) = cache.borrow().get(&config).cloned() {
            return glyph;
        }
        let (metrics, coverage) = font.rasterize_config(config);
        let glyph = Rc::new(GlyphBitmap { metrics, coverage });
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_GLYPHS {
            cache.clear();
        }
        cache.insert(config, Rc::clone(&glyph));
        glyph
    })
}

/// Rasterize a run into an atlas bitmap.
///
/// The size and the wrap width are the exact values the element measured with
/// (in physical pixels), not rounded ones: a run rasterized at a size or a
/// width the measurement never used can break its lines somewhere else and
/// produce a block taller than the box it was measured into.
pub(super) fn rasterize_text(
    text: &str,
    font_size: f32,
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
    let chain = font_chain(font, text);
    let layout = layout_text(text, font_size, max_width, line_height, &chain);

    let glyphs = layout.glyphs();
    // The quad spans the block's line box, so the drawn ink lands exactly where
    // `measure_text_family` sized the element (ascents, descents and line gaps
    // are all accounted for by `layout.height`).
    let line_box_height = layout
        .height()
        .ceil()
        .max(font_size * line_height)
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
    //
    // The bitmaps come from the glyph cache: a new line shares almost every
    // shape with the lines above it, and only the glyphs it is the first to use
    // are rasterized here.
    let rasterized: Vec<(f32, f32, Rc<GlyphBitmap>)> = glyphs
        .iter()
        .map(|glyph| {
            (
                glyph.x,
                glyph.y,
                glyph_bitmap(chain[glyph.font_index], glyph.key),
            )
        })
        .collect();
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y, glyph) in &rasterized {
        let metrics = &glyph.metrics;
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x + metrics.width as f32);
        max_y = max_y.max(*y + metrics.height as f32);
    }

    // Anchor the bitmap at the line-box origin so the quad aligns with the
    // element box; glyphs that overhang above/left still fit instead of being
    // clipped.
    let left = min_x.min(0.0).floor();
    let top = min_y.min(0.0).floor();
    let width = ((max_x - left).ceil() as u32).max(1);
    let height = ((line_box_height.max(max_y - top).ceil()).max(1.0)) as u32;

    let mut atlas = vec![0u8; (width * height) as usize];
    for (x, y, glyph) in &rasterized {
        let metrics = &glyph.metrics;
        if metrics.width == 0 || metrics.height == 0 {
            continue;
        }
        let x_offset = (*x - left).floor() as i32;
        let y_offset = (*y - top).floor() as i32;
        for row in 0..metrics.height {
            for col in 0..metrics.width {
                let src = row * metrics.width + col;
                let dst_x = x_offset + col as i32;
                let dst_y = y_offset + row as i32;
                if dst_x < 0 || dst_y < 0 || dst_x >= width as i32 || dst_y >= height as i32 {
                    continue;
                }
                let dst = (dst_y * width as i32 + dst_x) as usize;
                atlas[dst] = atlas[dst].saturating_add(glyph.coverage[src]);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A run holding a character the bundled face omits lays that character out
    /// through a system fallback, so it draws ink instead of the empty
    /// `.notdef` box. The test skips on a machine with no fallback face.
    #[test]
    fn a_glyph_outside_the_bundled_face_uses_a_fallback() {
        let Some(set) = font_set() else {
            return;
        };
        if fallback_fonts().is_empty() {
            return;
        }
        let primary = set.select(FontWeight::Regular, FontFamily::System, false);
        assert!(
            !primary.has_glyph('→'),
            "Roboto is the primary prose face and has no arrow"
        );
        let chain = font_chain(primary, "a → b");
        assert!(
            chain.len() > 1,
            "an arrow pulls in the fallback faces, got {} face(s)",
            chain.len()
        );

        let (_, atlas, width, height) =
            rasterize_text("→", 13.0, FontWeight::Regular, false, false, 100.0, 1.2)
                .expect("the arrow rasterizes");
        assert!(width > 0 && height > 0);
        assert!(
            atlas.iter().any(|coverage| *coverage > 0),
            "the fallback draws the arrow's ink"
        );
    }

    /// A line that scrolls into view shares almost every glyph with the lines
    /// already on screen; only the shapes it is the first to use are
    /// rasterized. This is what keeps a continuous scroll from re-parsing
    /// outlines for every new line.
    #[test]
    fn a_line_reuses_the_glyphs_the_lines_before_it_rasterized() {
        if font_set().is_none() {
            return;
        }
        GLYPH_CACHE.with(|cache| cache.borrow_mut().clear());
        let first = rasterize_text(
            "let value = 42;",
            12.0,
            FontWeight::Regular,
            true,
            false,
            f32::INFINITY,
            1.35,
        )
        .expect("the first line rasterizes");
        let after_first = GLYPH_CACHE.with(|cache| cache.borrow().len());
        assert!(after_first > 0, "the first line fills the glyph cache");

        let second = rasterize_text(
            "let answer = 7;",
            12.0,
            FontWeight::Regular,
            true,
            false,
            f32::INFINITY,
            1.35,
        )
        .expect("the second line rasterizes");
        let after_second = GLYPH_CACHE.with(|cache| cache.borrow().len());
        assert!(
            after_second - after_first < after_first,
            "the second line shares most of its glyphs: {} new against {}",
            after_second - after_first,
            after_first
        );
        assert!(
            first.2 > 0 && first.3 > 0 && second.2 > 0 && second.3 > 0,
            "both lines have real coverage"
        );
    }

    /// Every script the fallback chain can name draws real ink. A face that
    /// maps a character but rasterizes no outline (a colour font) is skipped,
    /// so a character only has to draw through a face that actually covers it.
    #[test]
    fn scripts_outside_the_bundled_faces_rasterize_ink() {
        if font_set().is_none() {
            return;
        }
        let candidates = fallback_fonts();
        if candidates.is_empty() {
            return;
        }
        // Latin diacritics, Greek, Cyrillic, Hebrew, Arabic, Devanagari, CJK,
        // arrows, checks, box drawing, braille and emoji: one representative
        // each.
        let samples = "șțăîâĂÂÎȘȚαβΩЖДЯאבגابتहिन्दी中文日本語한국어→←↑↓✓✗⚠◆┃▸─│┌┘⠋⠙⠹😀🙂👍";
        for ch in samples.chars() {
            let covered = candidates.iter().any(|font| draws_glyph(font, ch));
            if !covered {
                continue;
            }
            let mut text = String::new();
            text.push(ch);
            let (_, atlas, width, height) =
                rasterize_text(&text, 13.0, FontWeight::Regular, false, false, 100.0, 1.2)
                    .unwrap_or_else(|| panic!("{ch:?} must rasterize"));
            assert!(width > 0 && height > 0, "{ch:?} has an empty box");
            assert!(
                atlas.iter().any(|coverage| *coverage > 0),
                "{ch:?} is covered by a fallback but drew no ink"
            );
        }
    }

    /// The bold face reaches the atlas: the same run rasterized at `Bold` is a
    /// different bitmap from `Regular`, and both hold ink. This is the check
    /// that a `with_weight(Bold)` in the transcript survives measurement,
    /// layout and rasterization instead of collapsing to the regular face.
    #[test]
    fn bold_reaches_the_atlas_bitmap() {
        let (_, regular, regular_w, regular_h) =
            rasterize_text("Thought", 13.0, FontWeight::Regular, true, false, 200.0, 1.2)
                .expect("regular text rasterizes");
        let (_, bold, bold_w, bold_h) =
            rasterize_text("Thought", 13.0, FontWeight::Bold, true, false, 200.0, 1.2)
                .expect("bold text rasterizes");
        assert!(regular.iter().any(|coverage| *coverage > 0));
        assert!(bold.iter().any(|coverage| *coverage > 0));
        assert!(
            regular_w != bold_w || regular_h != bold_h || regular != bold,
            "bold must not reuse the regular bitmap"
        );
    }
}
