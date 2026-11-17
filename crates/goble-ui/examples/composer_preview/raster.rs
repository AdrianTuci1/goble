//! The CPU rasterizer behind the headless preview examples.
//!
//! This module is deliberately reusable: it is plain Rust over `goble_ui`'s
//! public API plus `fontdue`, `resvg` and `image`, and it finds its assets
//! through a caller-supplied directory rather than its own crate's
//! `CARGO_MANIFEST_DIR`. Another crate can include the same file with
//! `#[path]` and render its own element tree, e.g. an app-side sidebar preview:
//!
//! ```ignore
//! #[path = "../../crates/goble-ui/examples/composer_preview/raster.rs"]
//! mod raster;
//!
//! fn main() -> anyhow::Result<()> {
//!     use goble_ui::elements::{AppContext, Element};
//!     let assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
//!         .join("../crates/goble-ui/assets");
//!     let app = AppContext::default();
//!     let element: Box<dyn Element> = build_sidebar(&app);
//!     raster::render_scene_to_png(
//!         &assets_dir,
//!         &app,
//!         element,
//!         280.0,  // logical width
//!         900.0,  // logical height
//!         2.0,    // device pixel ratio
//!         std::path::Path::new("/tmp/goble-work/out/sidebar.png"),
//!     )
//! }
//! ```
//!
//! The including crate needs `anyhow`, `fontdue = "0.9"`, `resvg = "0.47"` and
//! `image = { version = "0.25", default-features = false, features = ["png"] }`
//! as (dev-)dependencies, plus `goble-ui` itself.
//!
//! The rasterizer mirrors the wgpu renderer in
//! `crates/goble-ui/src/platform/wgpu_render_engine/`:
//!
//! - fills and strokes are analytic rounded-rectangle SDFs with the same
//!   `softness = 0.7` physical-pixel anti-aliasing (stroke inset from the
//!   rect's outer edge, exactly like `RECT_SHADER`);
//! - text is fontdue layout at `font_size * scale`, with the same bundled
//!   Roboto/Hack faces and the same fallback chain, drawn from each glyph's
//!   rasterized coverage (the atlas path in `platform/text_atlas`);
//! - icons are the bundled SVG assets rasterized with resvg and tinted with the
//!   command's colour, fit and centred in a `size` box (the icon atlas path);
//! - clips become an intersected pixel scissor; the background is the theme's
//!   `Bg` token;
//! - blending happens in linear light, like the `Rgba8UnormSrgb` surface.
//!
//! Known limits (deliberate; they do not affect the composer/sidebar scenes):
//! colour emoji are not drawn (only the monochrome bundled fallbacks are
//! loaded), `DrawImage` samples nearest-neighbour, and hover states are not
//! painted because a headless `PaintContext` has no pointer.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};

use goble_ui::color::ColorU;
use goble_ui::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use goble_ui::geometry::{rectf, vec2f, PointF, RectF, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::theme::{ColorToken, FontFamily};

// ---------------------------------------------------------------------------
// Bundled assets (fonts + icon SVGs)
// ---------------------------------------------------------------------------

pub struct PreviewAssets {
    fonts: FontBook,
    icon_dir: PathBuf,
    /// device-pixel icon size -> rasterized coverage, keyed by (name, cell px).
    icons: RefCell<HashMap<(String, u32), Rc<IconBitmap>>>,
}

impl PreviewAssets {
    /// Load the bundled fonts and remember the crate's `assets/icons`
    /// directory. `assets_dir` is goble-ui's `crates/goble-ui/assets`.
    pub fn load(assets_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            fonts: FontBook::load(&assets_dir.join("fonts"))?,
            icon_dir: assets_dir.join("icons"),
            icons: RefCell::new(HashMap::new()),
        })
    }

    fn icon_path(&self, name: &str) -> Option<PathBuf> {
        // The atlas maps the project explorer's per-language icons to
        // `assets/icons/file_type/<language>.svg`; every other canonical name
        // is its own file.
        if let Some(language) = name.strip_prefix("file-") {
            let candidate = self
                .icon_dir
                .join("file_type")
                .join(format!("{language}.svg"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let candidate = self.icon_dir.join(format!("{name}.svg"));
        candidate.is_file().then_some(candidate)
    }

    fn icon(&self, name: &str, cell_px: u32) -> Option<Rc<IconBitmap>> {
        if let Some(bitmap) = self.icons.borrow().get(&(name.to_string(), cell_px)) {
            return Some(Rc::clone(bitmap));
        }
        let path = self.icon_path(name)?;
        let bitmap = Rc::new(IconBitmap::render(&path, cell_px)?);
        self.icons
            .borrow_mut()
            .insert((name.to_string(), cell_px), Rc::clone(&bitmap));
        Some(bitmap)
    }
}

/// An SVG icon rasterized to an alpha coverage bitmap, fit inside a
/// `cell_px`-square box like the icon atlas' 64px cells.
struct IconBitmap {
    width: u32,
    height: u32,
    /// The icon's aspect ratio inside the square cell: the atlas centres the
    /// rasterized SVG in its 64px cell and the renderer scales that cell to the
    /// command's `size`, so the drawn icon is centred within `size`.
    coverage: Vec<u8>,
}

impl IconBitmap {
    fn render(path: &Path, cell_px: u32) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let options = resvg::usvg::Options::default();
        let tree = resvg::usvg::Tree::from_data(&data, &options).ok()?;
        let size = tree.size();
        let cell = cell_px.max(1) as f32;
        let scale = cell / size.width().max(size.height());
        let width = (size.width() * scale).ceil().max(1.0) as u32;
        let height = (size.height() * scale).ceil().max(1.0) as u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        let coverage = pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect();
        Some(Self {
            width,
            height,
            coverage,
        })
    }
}

// ---------------------------------------------------------------------------
// Bundled fonts (the same faces `platform/text_atlas` draws with)
// ---------------------------------------------------------------------------

struct FontBook {
    regular: Font,
    medium: Font,
    bold: Font,
    semibold: Font,
    italic: Font,
    mono_regular: Font,
    mono_bold: Font,
    mono_italic: Font,
    mono_bold_italic: Font,
    /// Tried after the primary face, in coverage order: the system faces the
    /// text atlas chains, then the bundled monochrome Noto Emoji (macOS' colour
    /// emoji face cannot be rasterized to coverage).
    fallbacks: Vec<Font>,
}

impl FontBook {
    fn load(dir: &Path) -> anyhow::Result<Self> {
        let face = |name: &str| -> anyhow::Result<Font> {
            let path = dir.join(name);
            let bytes = std::fs::read(&path)
                .map_err(|error| anyhow::anyhow!("reading {}: {error}", path.display()))?;
            Font::from_bytes(bytes, FontSettings::default())
                .map_err(|error| anyhow::anyhow!("parsing {}: {error}", path.display()))
        };
        let mut fallbacks = Vec::new();
        for path in fallback_paths() {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                    fallbacks.push(font);
                }
            }
        }
        if let Ok(bytes) = std::fs::read(dir.join("noto-emoji/NotoEmoji-Regular.ttf")) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                fallbacks.push(font);
            }
        }
        Ok(Self {
            regular: face("roboto/Roboto-Regular.ttf")?,
            medium: face("roboto/Roboto-Medium.ttf")?,
            bold: face("roboto/Roboto-Bold.ttf")?,
            semibold: face("roboto/RobotoFlex-Semibold.ttf")?,
            italic: face("roboto/Roboto-Italic.ttf")?,
            mono_regular: face("hack/Hack-Regular.ttf")?,
            mono_bold: face("hack/Hack-Bold.ttf")?,
            mono_italic: face("hack/Hack-Italic.ttf")?,
            mono_bold_italic: face("hack/Hack-BoldItalic.ttf")?,
            fallbacks,
        })
    }

    fn primary(&self, weight: FontWeight, family: FontFamily, italic: bool) -> &Font {
        match family {
            FontFamily::Mono => match (italic, weight) {
                (true, FontWeight::Bold) => &self.mono_bold_italic,
                (true, _) => &self.mono_italic,
                (false, FontWeight::Bold) => &self.mono_bold,
                (false, _) => &self.mono_regular,
            },
            _ => match (italic, weight) {
                (true, _) => &self.italic,
                (false, FontWeight::SemiBold) => &self.semibold,
                (false, FontWeight::Medium) => &self.medium,
                (false, FontWeight::Bold) => &self.bold,
                (false, _) => &self.regular,
            },
        }
    }

    fn chain<'a>(&'a self, primary: &'a Font, text: &str) -> Vec<&'a Font> {
        let mut chain = Vec::with_capacity(1 + self.fallbacks.len());
        chain.push(primary);
        if text
            .chars()
            .any(|ch| !ch.is_whitespace() && !draws_glyph(primary, ch))
        {
            chain.extend(self.fallbacks.iter());
        }
        chain
    }
}

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
        ]
    }
    #[cfg(target_os = "windows")]
    {
        vec![
            "C:\\Windows\\Fonts\\consola.ttf",
            "C:\\Windows\\Fonts\\seguisym.ttf",
            "C:\\Windows\\Fonts\\arialuni.ttf",
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Element -> commands -> PNG
// ---------------------------------------------------------------------------

pub fn layout_size(element: &mut Box<dyn Element>, max: Vector2F, app: &AppContext) -> Vector2F {
    let mut ctx = LayoutContext::default();
    element.layout(SizeConstraint::loose(max), &mut ctx, app)
}

pub fn render_commands(
    element: &mut Box<dyn Element>,
    max: Vector2F,
    app: &AppContext,
) -> Vec<RenderCommand> {
    let _ = layout_size(element, max, app);
    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
    element.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// Rasterizes a command list onto a `Bg`-cleared canvas and writes it as an
/// RGBA PNG. Prints a one-line summary (path, pixels, command counts) so a
/// batch script's output says what it produced.
pub fn rasterize_to_png(
    assets: &PreviewAssets,
    app: &AppContext,
    commands: &[RenderCommand],
    canvas: Vector2F,
    scale: f32,
    background: Option<ColorU>,
    out: &Path,
) -> anyhow::Result<()> {
    let counts = goble_ui::test_util::command_counts(commands);
    let mut raster = Rasterizer::new(canvas, scale, app, assets, background);
    raster.render(commands);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    raster.save(out)?;
    println!(
        "wrote {} ({}x{} px, scale {scale}, {} commands: {} fill, {} stroke, {} text, {} icon, {} clip)",
        out.display(),
        raster.width,
        raster.height,
        commands.len(),
        counts.fill_rect + counts.fill_rect_fade,
        counts.stroke_rect,
        counts.draw_text,
        counts.draw_icon,
        counts.clip_rect,
    );
    for name in &raster.unknown_icons {
        eprintln!("warning: no SVG asset for icon {name:?}; skipped");
    }
    if raster.missing_glyphs > 0 {
        eprintln!(
            "warning: {} glyphs had no face in the fallback chain; skipped",
            raster.missing_glyphs
        );
    }
    Ok(())
}

/// Lays `element` out in a `canvas`-sized scene, paints it, rasterizes the
/// command list and writes `out` as a PNG at `scale` device pixels per point.
///
/// `background` overrides the page colour; `None` uses the theme's `Bg`. This is
/// the low-level entry point: `canvas` is the exact logical scene size, so the
/// caller owns framing.
pub fn render_to_png(
    assets: &PreviewAssets,
    app: &AppContext,
    element: &mut Box<dyn Element>,
    canvas: Vector2F,
    scale: f32,
    background: Option<ColorU>,
    out: &Path,
) -> anyhow::Result<()> {
    let commands = render_commands(element, canvas, app);
    rasterize_to_png(assets, app, &commands, canvas, scale, background, out)
}

/// The simple entry point for another crate's preview: lay `element` out in a
/// `width` x `height` logical scene, rasterize it at `scale` device pixels per
/// point over the theme's `Bg`, and write `out` as a PNG.
///
/// ```ignore
/// raster::render_element_to_png(
///     &assets, &app, &mut sidebar, 280.0, 900.0, 2.0, None,
///     Path::new("/tmp/goble-work/out/sidebar.png"),
/// )?;
/// ```
pub fn render_element_to_png(
    assets: &PreviewAssets,
    app: &AppContext,
    element: &mut Box<dyn Element>,
    width: f32,
    height: f32,
    scale: f32,
    background: Option<ColorU>,
    out: &Path,
) -> anyhow::Result<()> {
    render_to_png(
        assets,
        app,
        element,
        vec2f(width, height),
        scale,
        background,
        out,
    )
}

/// Renders a scene whose open popovers paint above the element's origin, like
/// the composer's menus: `build` is called with the vertical headroom the
/// element should leave for what paints above it.
///
/// The first pass lays the block out with no headroom and measures where its
/// topmost command lands; the second pass wraps it in that headroom and sizes
/// the canvas to `headroom + content_height`. `content_height` is the block's
/// own measured height (`layout_size`).
pub fn render_framed_to_png(
    assets: &PreviewAssets,
    app: &AppContext,
    build: impl Fn(f32) -> Box<dyn Element>,
    width: f32,
    content_height: f32,
    scale: f32,
    background: Option<ColorU>,
    out: &Path,
) -> anyhow::Result<()> {
    let mut probe = build(0.0);
    let probe_commands = render_commands(&mut probe, vec2f(width, 4000.0), app);
    let headroom = (-commands_min_y(&probe_commands)).max(0.0);
    let mut root = build(headroom);
    let canvas = vec2f(width, headroom + content_height);
    render_to_png(assets, app, &mut root, canvas, scale, background, out)
}

/// One-shot convenience for another crate's example: load goble-ui's bundled
/// assets from `assets_dir` (the crate's `assets/` directory), lay `element`
/// out at `width` x `height` logical points, rasterize at `scale` over the
/// theme's `Bg` and write `out`.
///
/// The including crate needs `anyhow`, `fontdue = "0.9"`, `resvg = "0.47"` and
/// `image = { version = "0.25", default-features = false, features = ["png"] }`
/// as (dev-)dependencies, plus `goble-ui` itself.
pub fn render_scene_to_png(
    assets_dir: &Path,
    app: &AppContext,
    element: Box<dyn Element>,
    width: f32,
    height: f32,
    scale: f32,
    out: &Path,
) -> anyhow::Result<()> {
    let assets = PreviewAssets::load(assets_dir)?;
    let mut element: Box<dyn Element> = element;
    render_element_to_png(&assets, app, &mut element, width, height, scale, None, out)
}

/// The topmost `y` any command paints at. A popup opens above its trigger, so
/// a menu scene's commands reach negative `y`; [`render_framed_to_png`] uses
/// that to leave exactly enough headroom for the open panel instead of a
/// guessed margin.
pub fn commands_min_y(commands: &[RenderCommand]) -> f32 {
    let mut min_y = 0.0f32;
    for command in commands {
        let top = match command {
            RenderCommand::FillRect { rect, .. }
            | RenderCommand::FillRectFadeRight { rect, .. }
            | RenderCommand::StrokeRect { rect, .. }
            | RenderCommand::DrawImage { rect, .. } => rect.min_y(),
            RenderCommand::DrawText {
                origin,
                font_size,
                line_height,
                ..
            } => origin.y - font_size * line_height.max(0.0),
            RenderCommand::DrawIcon { origin, .. } => origin.y,
            RenderCommand::ClipRect(rect) => rect.min_y(),
            RenderCommand::PopClip => continue,
        };
        min_y = min_y.min(top);
    }
    min_y
}

// ---------------------------------------------------------------------------
// Rasterizer
// ---------------------------------------------------------------------------

/// The pixel format of the real surface: the renderer emits sRGB colours and
/// the command list blends them over a `Bg` page.
struct Rasterizer<'a> {
    scale: f32,
    width: usize,
    height: usize,
    /// RGBA8, straight alpha. The page is opaque (`Bg`), so the alpha channel
    /// stays 255 and the PNG is a plain screenshot of the scene.
    pixels: Vec<u8>,
    clip_stack: Vec<RectF>,
    assets: &'a PreviewAssets,
    unknown_icons: Vec<String>,
    missing_glyphs: usize,
}

impl<'a> Rasterizer<'a> {
    fn new(
        canvas: Vector2F,
        scale: f32,
        app: &AppContext,
        assets: &'a PreviewAssets,
        background: Option<ColorU>,
    ) -> Self {
        let width = (canvas.x * scale).round().max(1.0) as usize;
        let height = (canvas.y * scale).round().max(1.0) as usize;
        let bg = background.unwrap_or_else(|| app.theme.color(ColorToken::Bg));
        let mut pixels = vec![0u8; width * height * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[0] = bg.r;
            pixel[1] = bg.g;
            pixel[2] = bg.b;
            pixel[3] = 255;
        }
        Self {
            scale,
            width,
            height,
            pixels,
            clip_stack: Vec::new(),
            assets,
            unknown_icons: Vec::new(),
            missing_glyphs: 0,
        }
    }

    fn save(&self, out: &Path) -> anyhow::Result<()> {
        let buffer =
            image::RgbaImage::from_raw(self.width as u32, self.height as u32, self.pixels.clone())
                .ok_or_else(|| anyhow::anyhow!("invalid raster dimensions"))?;
        buffer
            .save(out)
            .map_err(|error| anyhow::anyhow!("writing {}: {error}", out.display()))
    }

    fn render(&mut self, commands: &[RenderCommand]) {
        for command in commands {
            match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } => self.fill_rect(*rect, *color, *corner_radius, false),
                RenderCommand::FillRectFadeRight {
                    rect,
                    color,
                    corner_radius,
                } => self.fill_rect(*rect, *color, *corner_radius, true),
                RenderCommand::StrokeRect {
                    rect,
                    color,
                    width,
                    corner_radius,
                } => self.stroke_rect(*rect, *color, *width, *corner_radius),
                RenderCommand::DrawText {
                    origin,
                    text,
                    font_size,
                    color,
                    max_width,
                    line_height,
                    font_weight,
                    font_family,
                    font_italic,
                } => self.draw_text(
                    *origin,
                    text,
                    *font_size,
                    *color,
                    *max_width,
                    *line_height,
                    *font_weight,
                    *font_family,
                    *font_italic,
                ),
                RenderCommand::DrawIcon {
                    origin,
                    name,
                    size,
                    color,
                } => self.draw_icon(*origin, name, *size, *color),
                RenderCommand::DrawImage {
                    rect,
                    width,
                    height,
                    data,
                    ..
                } => self.draw_image(*rect, *width, *height, data),
                RenderCommand::ClipRect(rect) => self.clip_stack.push(*rect),
                RenderCommand::PopClip => {
                    self.clip_stack.pop();
                }
            }
        }
    }

    /// The pixel bounds of the clip stack: the intersection of every clip in
    /// force, in physical pixels (the renderer's scissor is the same
    /// intersection — see `batch::clip_scissor`).
    fn clip_px(&self) -> Option<RectF> {
        let mut bounds: Option<RectF> = None;
        for clip in &self.clip_stack {
            let clip = scale_rect(*clip, self.scale);
            bounds = Some(match bounds {
                None => clip,
                Some(bounds) => intersect(bounds, clip),
            });
        }
        bounds
    }

    fn fill_rect(&mut self, rect: RectF, color: ColorU, radius: f32, fade_right: bool) {
        let rect = scale_rect(rect, self.scale);
        let radius = radius * self.scale;
        let clip = self.clip_px();
        let (x0, y0, x1, y1) = self.pixel_bounds(rect, clip);
        for py in y0..y1 {
            for px in x0..x1 {
                let cx = px as f32 + 0.5;
                let cy = py as f32 + 0.5;
                let distance = rounded_rect_distance(
                    cx - rect.min_x(),
                    cy - rect.min_y(),
                    rect.width(),
                    rect.height(),
                    radius,
                );
                let mut coverage = coverage_for(distance);
                if coverage <= 0.0 {
                    continue;
                }
                if fade_right {
                    let t = ((cx - rect.min_x()) / rect.width().max(0.001)).clamp(0.0, 1.0);
                    coverage *= t;
                }
                blend(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    px as i32,
                    py as i32,
                    color,
                    coverage,
                );
            }
        }
    }

    fn stroke_rect(&mut self, rect: RectF, color: ColorU, width: f32, radius: f32) {
        let rect = scale_rect(rect, self.scale);
        let radius = radius * self.scale;
        let width = width * self.scale;
        let clip = self.clip_px();
        let (x0, y0, x1, y1) = self.pixel_bounds(rect, clip);
        for py in y0..y1 {
            for px in x0..x1 {
                let cx = px as f32 + 0.5;
                let cy = py as f32 + 0.5;
                let distance = rounded_rect_distance(
                    cx - rect.min_x(),
                    cy - rect.min_y(),
                    rect.width(),
                    rect.height(),
                    radius,
                );
                // `RECT_SHADER`: the stroke is inset from the rect's outer edge
                // and anti-aliased on both of its edges.
                let outer = 1.0 - smoothstep(0.0, SOFTNESS, distance);
                let inner = smoothstep(0.0, SOFTNESS, distance + width);
                let coverage = outer * inner;
                if coverage <= 0.0 {
                    continue;
                }
                blend(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    px as i32,
                    py as i32,
                    color,
                    coverage,
                );
            }
        }
    }

    fn draw_text(
        &mut self,
        origin: Vector2F,
        text: &str,
        font_size: f32,
        color: ColorU,
        max_width: f32,
        line_height: f32,
        weight: FontWeight,
        family: FontFamily,
        italic: bool,
    ) {
        if text.is_empty() {
            return;
        }
        let size_px = font_size * self.scale;
        if size_px <= 0.0 {
            return;
        }
        let max_width_px = if max_width.is_finite() && max_width > 0.0 {
            Some(max_width * self.scale)
        } else {
            None
        };
        let line_height = if line_height > 0.0 { line_height } else { 1.2 };

        // Rasterize every glyph first, then blend: the font chain borrows the
        // font book, and blending borrows the pixel buffer.
        let glyphs = {
            let book = &self.assets.fonts;
            let primary = book.primary(weight, family, italic);
            let chain = book.chain(primary, text);
            let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
            layout.reset(&LayoutSettings {
                max_width: max_width_px,
                max_height: None,
                line_height,
                ..Default::default()
            });
            append_with_fallback(&mut layout, &chain, text, size_px);
            let mut glyphs: Vec<PlacedGlyph> = Vec::with_capacity(layout.glyphs().len());
            for glyph in layout.glyphs() {
                if glyph.char_data.is_control() {
                    continue;
                }
                let font = match chain.get(glyph.font_index) {
                    Some(font) => *font,
                    None => continue,
                };
                let (metrics, coverage) = font.rasterize_config(glyph.key);
                if metrics.width == 0 || metrics.height == 0 {
                    continue;
                }
                glyphs.push(PlacedGlyph {
                    x: origin.x * self.scale + glyph.x,
                    y: origin.y * self.scale + glyph.y,
                    width: metrics.width,
                    height: metrics.height,
                    coverage,
                });
            }
            glyphs
        };

        let clip = self.clip_px();
        for glyph in glyphs {
            let left = glyph.x;
            let top = glyph.y;
            for py in (top.floor() as i32)..((top + glyph.height as f32).ceil() as i32) {
                for px in (left.floor() as i32)..((left + glyph.width as f32).ceil() as i32) {
                    if px < 0 || py < 0 || px as usize >= self.width || py as usize >= self.height {
                        continue;
                    }
                    if let Some(clip) = clip {
                        if !clip.contains(PointF::new(px as f32 + 0.5, py as f32 + 0.5)) {
                            continue;
                        }
                    }
                    let coverage = sample_bilinear(
                        &glyph.coverage,
                        glyph.width,
                        glyph.height,
                        px as f32 + 0.5 - left,
                        py as f32 + 0.5 - top,
                    );
                    if coverage <= 0.0 {
                        continue;
                    }
                    blend(
                        &mut self.pixels,
                        self.width,
                        self.height,
                        px,
                        py,
                        color,
                        coverage / 255.0,
                    );
                }
            }
        }
    }

    fn draw_icon(&mut self, origin: Vector2F, name: &str, size: f32, color: ColorU) {
        let cell = (size * self.scale).round().max(1.0);
        let Some(bitmap) = self.assets.icon(name, cell as u32) else {
            if !self.unknown_icons.iter().any(|known| known == name) {
                self.unknown_icons.push(name.to_string());
            }
            return;
        };
        // The atlas centres the rasterized SVG in its `cell`-square glyph and the
        // renderer scales that glyph to the command's `size`; the drawn icon is
        // therefore centred inside `[origin, origin + size]`.
        let cell = cell as f32;
        let left = origin.x * self.scale + (cell - bitmap.width as f32) * 0.5;
        let top = origin.y * self.scale + (cell - bitmap.height as f32) * 0.5;
        let clip = self.clip_px();
        for py in (top.floor() as i32)..((top + bitmap.height as f32).ceil() as i32) {
            for px in (left.floor() as i32)..((left + bitmap.width as f32).ceil() as i32) {
                if px < 0 || py < 0 || px as usize >= self.width || py as usize >= self.height {
                    continue;
                }
                if let Some(clip) = clip {
                    if !clip.contains(PointF::new(px as f32 + 0.5, py as f32 + 0.5)) {
                        continue;
                    }
                }
                let coverage = sample_bilinear(
                    &bitmap.coverage,
                    bitmap.width as usize,
                    bitmap.height as usize,
                    px as f32 + 0.5 - left,
                    py as f32 + 0.5 - top,
                );
                if coverage <= 0.0 {
                    continue;
                }
                blend(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    px,
                    py,
                    color,
                    coverage / 255.0,
                );
            }
        }
    }

    fn draw_image(&mut self, rect: RectF, width: u32, height: u32, data: &[u8]) {
        if width == 0 || height == 0 || data.len() < (width * height * 4) as usize {
            return;
        }
        let rect = scale_rect(rect, self.scale);
        let clip = self.clip_px();
        let (x0, y0, x1, y1) = self.pixel_bounds(rect, clip);
        for py in y0..y1 {
            for px in x0..x1 {
                let u = (px as f32 + 0.5 - rect.min_x()) / rect.width().max(0.001);
                let v = (py as f32 + 0.5 - rect.min_y()) / rect.height().max(0.001);
                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                    continue;
                }
                let sx = ((u * width as f32) as usize).min(width as usize - 1);
                let sy = ((v * height as f32) as usize).min(height as usize - 1);
                let index = (sy * width as usize + sx) * 4;
                let color = ColorU::new(
                    data[index],
                    data[index + 1],
                    data[index + 2],
                    data[index + 3],
                );
                blend(
                    &mut self.pixels,
                    self.width,
                    self.height,
                    px as i32,
                    py as i32,
                    color,
                    color.a as f32 / 255.0,
                );
            }
        }
    }

    /// The pixel rectangle a command can touch: its own bounds, rounded out,
    /// clipped by the scissor and the canvas.
    fn pixel_bounds(&self, rect: RectF, clip: Option<RectF>) -> (usize, usize, usize, usize) {
        let mut bounds = rect;
        if let Some(clip) = clip {
            bounds = intersect(bounds, clip);
        }
        let x0 = bounds.min_x().floor().max(0.0) as usize;
        let y0 = bounds.min_y().floor().max(0.0) as usize;
        let x1 = (bounds.max_x().ceil().max(0.0) as usize).min(self.width);
        let y1 = (bounds.max_y().ceil().max(0.0) as usize).min(self.height);
        (x0.min(x1), y0.min(y1), x1, y1)
    }
}

/// One rasterized glyph and where its bitmap's top-left lands, in physical
/// pixels (fontdue bakes the bearings into `x`/`y`, so no extra offset).
struct PlacedGlyph {
    x: f32,
    y: f32,
    width: usize,
    height: usize,
    coverage: Vec<u8>,
}

/// The anti-aliasing softness the renderer's rect shader uses, in physical
/// pixels (`RECT_SHADER`'s `softness`).
const SOFTNESS: f32 = 0.7;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `1 - smoothstep(0, softness, distance)` for a rounded-rect SDF: 1 inside the
/// shape, 0 outside, an anti-aliased ramp across the edge.
fn coverage_for(distance: f32) -> f32 {
    1.0 - smoothstep(0.0, SOFTNESS, distance)
}

/// Signed distance to a rounded rectangle whose top-left is the local origin,
/// exactly the calculation `RECT_SHADER` does.
fn rounded_rect_distance(local_x: f32, local_y: f32, width: f32, height: f32, radius: f32) -> f32 {
    let radius = radius.clamp(0.0, width.min(height) * 0.5);
    let half_width = width * 0.5;
    let half_height = height * 0.5;
    let qx = (local_x - half_width).abs() - half_width + radius;
    let qy = (local_y - half_height).abs() - half_height + radius;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

fn scale_rect(rect: RectF, scale: f32) -> RectF {
    rectf(
        rect.min_x() * scale,
        rect.min_y() * scale,
        rect.width() * scale,
        rect.height() * scale,
    )
}

fn intersect(a: RectF, b: RectF) -> RectF {
    let min_x = a.min_x().max(b.min_x());
    let min_y = a.min_y().max(b.min_y());
    let max_x = a.max_x().min(b.max_x());
    let max_y = a.max_y().min(b.max_y());
    rectf(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}

/// Source-over compositing of `color` at `coverage` onto an opaque RGBA8 page.
///
/// The real surface is `Rgba8UnormSrgb`, so the GPU's blend happens in linear
/// light (the shaders hand it `ColorU::to_linear_f32()` values and the sRGB
/// framebuffer converts on write). Blending linearly here keeps anti-aliased
/// edges and translucent fills the same shade the window shows; a fully opaque
/// pixel is written straight from the command's colour.
fn blend(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: ColorU,
    coverage: f32,
) {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    let alpha = (color.a as f32 / 255.0) * coverage;
    if alpha <= 0.0 {
        return;
    }
    let index = (y * width + x) * 4;
    if alpha >= 0.999 {
        pixels[index] = color.r;
        pixels[index + 1] = color.g;
        pixels[index + 2] = color.b;
        pixels[index + 3] = 255;
        return;
    }
    let inverse = 1.0 - alpha;
    let src = [
        srgb_to_linear(color.r),
        srgb_to_linear(color.g),
        srgb_to_linear(color.b),
    ];
    for channel in 0..3 {
        let dst = srgb_to_linear(pixels[index + channel]);
        let linear = src[channel] * alpha + dst * inverse;
        pixels[index + channel] = linear_to_srgb(linear);
    }
    pixels[index + 3] = 255;
}

fn srgb_to_linear(value: u8) -> f32 {
    let c = value as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let c = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// Bilinear sample of a coverage bitmap, treating texel centres as whole
/// (i + 0.5) coordinates — the linear filter the text/icon atlases are sampled
/// with.
fn sample_bilinear(bitmap: &[u8], width: usize, height: usize, x: f32, y: f32) -> f32 {
    if width == 0 || height == 0 || bitmap.len() < width * height {
        return 0.0;
    }
    let x = x - 0.5;
    let y = y - 0.5;
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let sample = |ix: i32, iy: i32| -> f32 {
        if ix < 0 || iy < 0 || ix as usize >= width || iy as usize >= height {
            0.0
        } else {
            bitmap[iy as usize * width + ix as usize] as f32
        }
    };
    let x0 = x0 as i32;
    let y0 = y0 as i32;
    let top = sample(x0, y0) * (1.0 - fx) + sample(x0 + 1, y0) * fx;
    let bottom = sample(x0, y0 + 1) * (1.0 - fx) + sample(x0 + 1, y0 + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

// ---------------------------------------------------------------------------
// Text layout (the `platform/text_atlas` algorithm over the fallback chain)
// ---------------------------------------------------------------------------

/// Layout `text` through `fonts`, splitting at every character the current face
/// cannot draw and continuing with the first face that can — the same walk
/// `platform/text_atlas::fonts::layout_text_with_runs` does.
fn append_with_fallback(layout: &mut Layout, fonts: &[&Font], text: &str, font_size: f32) {
    let Some(first) = text.chars().next() else {
        return;
    };
    let face_for = |ch: char| -> usize {
        if ch.is_whitespace() || ch.is_control() {
            return 0;
        }
        if let Some(face) = fonts.iter().position(|font| draws_glyph(font, ch)) {
            return face;
        }
        if is_zero_width_ignorable(ch) {
            if let Some((face, _)) = fonts
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, font)| font.has_glyph(ch))
            {
                return face;
            }
        }
        0
    };
    let mut start = 0;
    let mut current = face_for(first);
    for (offset, ch) in text.char_indices().skip(1) {
        let face = face_for(ch);
        if face != current {
            layout.append(
                fonts,
                &TextStyle::new(&text[start..offset], font_size, current),
            );
            start = offset;
            current = face;
        }
    }
    layout.append(fonts, &TextStyle::new(&text[start..], font_size, current));
}

fn draws_glyph(font: &Font, ch: char) -> bool {
    if !font.has_glyph(ch) {
        return false;
    }
    let metrics = font.metrics(ch, 16.0);
    if metrics.width > 0 && metrics.height > 0 {
        return true;
    }
    is_blank_filler(ch)
}

fn is_blank_filler(ch: char) -> bool {
    matches!(ch, '\u{2800}' | '\u{3164}' | '\u{ffa0}')
}

fn is_zero_width_ignorable(ch: char) -> bool {
    matches!(
        ch,
        '\u{200b}'..='\u{200f}' | '\u{2060}'..='\u{206f}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}'
    )
}
