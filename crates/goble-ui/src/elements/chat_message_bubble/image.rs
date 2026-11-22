//! Markdown images in the transcript.
//!
//! A decoded local image is drawn at DSH's prose size: at most
//! [`IMAGE_MAX_WIDTH`] wide (or the lane's width when narrower) and
//! [`IMAGE_MAX_HEIGHT`] tall, preserving the file's aspect ratio. The corners
//! are cut with the pane-card technique, a 1 px theme border traces the same
//! rounded path, and the alt text follows as a quiet caption.
//!
//! Only local bytes are read: absolute paths, `file://` URLs and the relative
//! destinations a markdown `![alt](path)` parse produces (resolved against the
//! process's working directory). Anything with a network scheme is left to the
//! honest alt-text fallback in [`super::bubble`]; no request is ever made.
//!
//! Decoding is cached thread-locally by `(path, mtime, size)`. The transcript
//! rebuilds its element tree on every frame, so without the cache a single
//! visible screenshot would be decoded again for every paint; the key includes
//! the file's metadata so a rewritten file is decoded afresh.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use crate::color::ColorU;
use crate::elements::chat_content::ChatAction;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::render::Renderer;
use crate::theme::ColorToken;

/// An image never draws wider than this, even on a wide lane.
pub(super) const IMAGE_MAX_WIDTH: f32 = 360.0;
/// An image never draws taller than this; a tall screenshot shrinks to fit.
pub(super) const IMAGE_MAX_HEIGHT: f32 = 280.0;
/// How far outside the rounded image the corner-cut band reaches. The deepest
/// spandrel of a radius-8 corner is `8 * (sqrt(2) - 1) ≈ 3.3`, so four points
/// reach every corner. The band is clamped on a very small image.
const CORNER_CUT_BAND: f32 = 4.0;
/// The gap between the image and its alt caption.
const CAPTION_GAP: f32 = 6.0;
/// The most decoded images one thread keeps: a transcript rarely shows more,
/// and dropping the map at the cap keeps a long scroll from retaining every
/// screenshot it passed.
const IMAGE_CACHE_LIMIT: usize = 16;

/// What an authored image destination resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ImageTarget {
    /// A local file the element may read.
    Local(PathBuf),
    /// A network-style URL. It is never fetched and always falls back to the
    /// alt-text chip; a click still opens it.
    Remote(String),
}

impl ImageTarget {
    /// The action a click on the image or its fallback chip dispatches: a
    /// local file opens in a file pane, any other URL through the guarded
    /// external opener.
    pub(super) fn action(&self) -> ChatAction {
        match self {
            Self::Local(path) => ChatAction::OpenFile(path.to_string_lossy().into_owned()),
            Self::Remote(url) => ChatAction::OpenUrl(url.clone()),
        }
    }

    fn local_path(&self) -> Option<&Path> {
        match self {
            Self::Local(path) => Some(path),
            Self::Remote(_) => None,
        }
    }
}

/// Resolve one authored `![alt](destination)` target.
///
/// `file://` is a local URL; an absolute path is local as written; a relative
/// path (which is what markdown produces for a workspace screenshot) is joined
/// to the process's working directory; every other scheme is remote. Query and
/// fragment suffixes are ignored for local paths, matching the reference's
/// `![alt](path)` handling. A malformed percent escape leaves the URL remote
/// rather than guessing at a filename.
pub(super) fn resolve_image_target(raw: &str) -> ImageTarget {
    let raw = raw.trim();
    if raw.is_empty() {
        return ImageTarget::Remote(raw.to_string());
    }
    // The suffix is only meaningful for a Web URL; a local file name is read
    // up to it, as the reference's local-path resolver does.
    let candidate = raw.split(['?', '#']).next().unwrap_or(raw);
    if let Some(path) = file_url_path(candidate) {
        return ImageTarget::Local(path);
    }
    if candidate.starts_with("file:") {
        // A malformed `file:` URL names no readable local path; leave it to
        // the remote fallback rather than joining the scheme onto the cwd.
        return ImageTarget::Remote(raw.to_string());
    }
    if has_network_scheme(candidate) || candidate.starts_with("//") {
        return ImageTarget::Remote(raw.to_string());
    }
    let decoded = percent_decode(candidate).unwrap_or_else(|| candidate.to_string());
    let path = PathBuf::from(decoded);
    let path = if path.is_absolute() {
        path
    } else if let Some(home) = home_relative(&path) {
        home
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    };
    ImageTarget::Local(path)
}

/// Expand a leading `~/` against `HOME`, so a markdown path a shell would
/// accept resolves the same way.
fn home_relative(path: &Path) -> Option<PathBuf> {
    let text = path.to_str()?;
    let rest = text.strip_prefix("~/")?;
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(rest))
}

/// `file:///abs`, `file://localhost/abs` and `file:/abs` all name a local path.
fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url
        .strip_prefix("file://")
        .or_else(|| url.strip_prefix("file:"))?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest)?;
    let path = PathBuf::from(decoded);
    path.is_absolute().then_some(path)
}

/// `scheme:` other than `file:` means a remote URL, never a Windows drive
/// letter (`C:` is a single letter before the colon).
fn has_network_scheme(candidate: &str) -> bool {
    let Some((scheme, _)) = candidate.split_once(':') else {
        return false;
    };
    if scheme.len() < 2 && scheme.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    let mut chars = scheme.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

/// Decode `%XX` escapes once, as a browser does for a local media URL.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(bytes.get(index + 1).copied()?)?;
            let low = hex(bytes.get(index + 2).copied()?)?;
            out.push(high << 4 | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The decoded RGBA8 pixels and the metadata the renderer's texture cache needs.
#[derive(Clone)]
pub(super) struct DecodedImage {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) data: Arc<[u8]>,
    /// Stable per (path, mtime, size): the renderer re-uploads only when this
    /// changes.
    pub(super) frame_seq: u64,
    /// The texture cache's key, distinct for each file.
    pub(super) source: String,
}

/// `(path, mtime nanoseconds, file length)`: the identity of one decode.
#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    path: PathBuf,
    modified: u64,
    length: u64,
}

thread_local! {
    static IMAGE_CACHE: RefCell<HashMap<CacheKey, Arc<DecodedImage>>> =
        RefCell::new(HashMap::new());
}

/// Decode a local image, or return the cached pixels when the file has not
/// changed since the last frame.
pub(super) fn decoded_image(target: &ImageTarget) -> Option<Arc<DecodedImage>> {
    let path = target.local_path()?;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let key = CacheKey {
        path: path.to_path_buf(),
        modified,
        length: metadata.len(),
    };
    if let Some(cached) = IMAGE_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(cached);
    }

    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif)
    ) {
        return None;
    }
    let decoded = reader.decode().ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    if width == 0 || height == 0 {
        return None;
    }
    let rgba = decoded.to_rgba8();
    let frame_seq = modified
        .wrapping_mul(31)
        .wrapping_add(metadata.len().rotate_left(17))
        .max(1);
    let image = Arc::new(DecodedImage {
        width,
        height,
        data: Arc::from(rgba.into_raw()),
        frame_seq,
        source: path.to_string_lossy().into_owned(),
    });
    IMAGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= IMAGE_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&image));
    });
    Some(image)
}

/// The image surface itself: one decoded file at DSH's prose size, with the
/// rounded cut and a hairline border. Click handling lives on a wrapper so the
/// figure can also carry a non-clickable caption.
pub(super) struct ChatImage {
    decoded: Arc<DecodedImage>,
    /// The conversation surface the corners are cut back to: the user bubble
    /// when the image is inside one, otherwise the pane's own background.
    backdrop: ColorU,
    radius: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatImage {
    pub(super) fn new(decoded: Arc<DecodedImage>, user_bubble: bool, app: &AppContext) -> Self {
        let backdrop = if user_bubble {
            app.theme.color(ColorToken::SurfaceRaised)
        } else {
            app.theme.color(ColorToken::Bg)
        };
        Self {
            decoded,
            backdrop,
            radius: app.theme.radius_px().max(4.0),
            size: None,
            origin: None,
        }
    }

    /// The drawn size for a `lane`-wide row: the file's aspect ratio inside
    /// [`IMAGE_MAX_WIDTH`] × [`IMAGE_MAX_HEIGHT`], never upscaled.
    fn fitted_size(&self, lane: f32) -> Vector2F {
        let width = self.decoded.width as f32;
        let height = self.decoded.height as f32;
        if width <= 0.0 || height <= 0.0 {
            return Vector2F::zero();
        }
        let max_width = IMAGE_MAX_WIDTH.min(lane.max(0.0));
        if max_width <= 0.0 {
            return Vector2F::zero();
        }
        let scale = (max_width / width).min(IMAGE_MAX_HEIGHT / height).min(1.0);
        vec2f((width * scale).max(1.0), (height * scale).max(1.0))
    }
}

impl Element for ChatImage {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = self.fitted_size(constraint.max.x);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else {
            return;
        };
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        renderer.draw_image(
            rect,
            self.decoded.source.clone(),
            self.decoded.width,
            self.decoded.height,
            self.decoded.frame_seq,
            Arc::clone(&self.decoded.data),
        );
        paint_corner_cut(renderer, rect, self.radius, self.backdrop);
        // The border traces the same rounded path after the cut, so the image's
        // corners read round instead of square.
        renderer.stroke_rect(rect, app.theme.color(ColorToken::Border), 1.0, self.radius);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

/// The pane-card corner cut, applied to one image: stroke a rectangle grown by
/// the band with the background the image sits on. The renderer strokes inward
/// from the given rectangle's edge, so the band lands exactly outside the
/// image's rounded path and only shows at the four corners.
fn paint_corner_cut(renderer: &mut Renderer, image: RectF, radius: f32, backdrop: ColorU) {
    let band = CORNER_CUT_BAND
        .min(image.width() * 0.5)
        .min(image.height() * 0.5)
        .max(0.0);
    if band <= 0.0 {
        return;
    }
    renderer.stroke_rect(
        rectf(
            image.min_x() - band,
            image.min_y() - band,
            image.width() + band * 2.0,
            image.height() + band * 2.0,
        ),
        backdrop,
        band,
        radius + band,
    );
}

/// The image plus its quiet alt caption, laid out as one prose block. Only the
/// image (when the caller wrapped it in a click target) is interactive; the
/// caption stays inert so a text selection over it keeps working.
pub(super) struct ImageFigure {
    image: Box<dyn Element>,
    caption: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ImageFigure {
    pub(super) fn new(image: Box<dyn Element>, caption: Option<Box<dyn Element>>) -> Self {
        Self {
            image,
            caption,
            size: None,
            origin: None,
        }
    }
}

impl Element for ImageFigure {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let image = self.image.layout(constraint, ctx, app);
        let caption = self.caption.as_mut().map(|caption| {
            caption.layout(
                SizeConstraint::new(vec2f(0.0, 0.0), vec2f(image.x, f32::INFINITY)),
                ctx,
                app,
            )
        });
        let caption_height = caption.map(|size| size.y).unwrap_or(0.0);
        let gap = if caption.is_some() { CAPTION_GAP } else { 0.0 };
        let width = image.x.max(caption.map(|size| size.x).unwrap_or(0.0));
        let size = vec2f(width, image.y + gap + caption_height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.image.paint(origin, ctx, app);
        if let Some(caption) = self.caption.as_mut() {
            let image_height = self.image.size().map(|size| size.y).unwrap_or(0.0);
            caption.paint(origin + vec2f(0.0, image_height + CAPTION_GAP), ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        // A click on the picture is the picture's; the caption below it is not
        // part of the hit target.
        self.image.dispatch_event(event, ctx, app)
    }
}
