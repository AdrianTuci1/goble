//! In-band terminal images: the kitty graphics protocol and sixel.
//!
//! Both protocols are escape sequences the VT parser has no grid cell for. The
//! image tap sits in front of the parser and lifts them out of the byte stream,
//! the way [`crate::hooks::HookTap`] lifts our own `DCS` envelope: an image is
//! decoded into a typed [`ImageEvent`], everything else is passed through
//! untouched.
//!
//! * kitty is an `APC` string, `ESC _ G <control>;<payload> ESC \`, with the
//!   payload base64. Formats are raw RGB (`f=24`), raw RGBA (`f=32`) and PNG
//!   (`f=100`). Chunked transmissions (`m=1`) are joined by image id. The
//!   actions `a=t` (transmit), `a=T` (transmit and display), `a=p` (display an
//!   image this tap received earlier) and `a=d` (delete) are handled; `a=q`
//!   queries are not answered.
//! * sixel is a `DCS` string, `ESC P <params> q <data> ESC \`. The data is a
//!   6-bit palette bitmap; [`decode_sixel`] implements the palette and raster
//!   operators rather than depending on a sixel crate.
//!
//! Decoded pixels are held in a bounded [`ImageCache`] (count and byte limits)
//! so a program cannot grow a pane's heap without bound. A transmission is also
//! retained by the tap, under the same count and byte budget, until `a=p`
//! places it or a delete removes it. The cache keeps each image's absolute
//! content row, which is what makes a placement scroll with the text around
//! it: the renderer maps that row through the viewport's current scroll offset.
//!
//! What is not implemented, so a program relying on it draws nothing rather
//! than a wrong image:
//!
//! * kitty's compressed (`o=z`), file (`t=f`, `t=t`) and shared-memory (`t=s`)
//!   transmission media; only base64 pixels carried in the APC string decode.
//! * kitty placement details: a placement is anchored to the cursor cell and
//!   keyed by image id, so a second `a=p` of the same image replaces the first
//!   rather than adding another copy. Placement ids (`p=`), pixel offsets
//!   (`x=`, `y=`), source cropping and z-order (`z=`) are ignored, so a delete
//!   that names only a placement id cannot target one placement.
//! * kitty Unicode placeholders, and the terminal-capability replies its
//!   queries probe for.
//! * kitty delete intersection specs (`d=x`, `d=y`, `d=z`). The image and
//!   placement specs `d=a`, `d=A`, `d=i`, `d=I`, `d=p` and `d=P` are
//!   understood.
//! * sixel scrolling mode and the sixel geometry hints beyond the operators
//!   [`decode_sixel`] lists.
//!
//! A screen clears its placement cache when it enters or leaves the alternate
//! screen, because the primary and alternate grids are separate surfaces, and
//! on reset. Placements keep their absolute content row, so they scroll with
//! text, but they do not reflow when a resize rewraps the rows around them.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Images a pane keeps unless the caller configures another bound.
pub const DEFAULT_IMAGE_CACHE_IMAGES: usize = 32;
/// Decoded bytes a pane keeps unless the caller configures another bound.
pub const DEFAULT_IMAGE_CACHE_BYTES: usize = 32 * 1024 * 1024;
/// The largest sequence the tap buffers before abandoning it.
const MAX_SEQUENCE_BYTES: usize = 16 * 1024 * 1024;
/// A cap on either dimension the sixel decoder will materialise.
const MAX_SIXEL_DIMENSION: usize = 16_384;
/// A cap on the pixels one image may decode to. The cache's byte budget keeps
/// retained images bounded; this stops a compressed stream from allocating far
/// more than the cache could ever hold before that check runs.
const MAX_IMAGE_PIXELS: u64 = 8_000_000;
const MAX_SIXEL_PIXELS: usize = 8_000_000;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const ST_FINAL: u8 = b'\\';

/// Which protocol and pixel encoding an image arrived in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// kitty `f=100`: the payload is a PNG file.
    KittyPng,
    /// kitty `f=24`: the payload is raw RGB triples.
    KittyRgb,
    /// kitty `f=32`: the payload is raw RGBA quadruples.
    KittyRgba,
    /// A sixel `DCS` stream, decoded to RGBA.
    Sixel,
}

/// One image the tap decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageEvent {
    /// The protocol image id. kitty's `i` when it named one; otherwise a
    /// monotonically increasing id from the tap.
    pub id: u32,
    pub format: ImageFormat,
    /// Pixel dimensions after decoding. For raw kitty formats these are the
    /// transmitted `s`/`v`; for PNG the file's own dimensions are authoritative.
    pub width: u32,
    pub height: u32,
    /// The bytes as they arrived (base64-decoded for kitty, the sixel data for
    /// sixel). Kept for callers that want the original encoding.
    pub payload: Arc<[u8]>,
    /// The image as RGBA8 pixels, `width * height * 4` bytes, when the format
    /// could be decoded.
    pub rgba: Option<Arc<[u8]>>,
    /// kitty `c`: how many cells wide the placement is.
    pub columns: Option<usize>,
    /// kitty `r`: how many cells tall the placement is.
    pub rows: Option<usize>,
    /// Whether the sequence displays the image (`a=T` or `a=p`), not only
    /// transmits it.
    pub display: bool,
}

impl ImageEvent {
    /// Whether this image has pixels the renderer can upload.
    pub fn is_renderable(&self) -> bool {
        self.rgba
            .as_ref()
            .is_some_and(|rgba| rgba.len() == self.width as usize * self.height as usize * 4)
    }
}

/// A decoded image, where it was placed, and its cache order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedImage {
    /// The id the placement used.
    pub id: u32,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub payload: Arc<[u8]>,
    pub rgba: Arc<[u8]>,
    /// Absolute content row (history rows included) the image starts on.
    pub row: usize,
    /// The cursor column when the image was placed.
    pub column: usize,
    pub columns: Option<usize>,
    pub rows: Option<usize>,
    /// Monotonic insertion order. The renderer uses it as the texture's
    /// revision, so a re-transmitted id re-uploads rather than showing the old
    /// pixels.
    pub sequence: u64,
}

/// An image mapped into the viewport the renderer is about to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePlacement {
    pub id: u32,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    /// Row within the viewport, top row first.
    pub row: usize,
    /// Column within the viewport.
    pub column: usize,
    pub columns: Option<usize>,
    pub rows: Option<usize>,
    pub rgba: Arc<[u8]>,
    /// The image's cache sequence, used as the texture revision.
    pub sequence: u64,
}

/// A bounded, insertion-ordered cache of decoded images for one pane.
#[derive(Debug, Clone)]
pub struct ImageCache {
    images: VecDeque<CachedImage>,
    max_images: usize,
    max_bytes: usize,
    bytes: usize,
    next_sequence: u64,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new(DEFAULT_IMAGE_CACHE_IMAGES, DEFAULT_IMAGE_CACHE_BYTES)
    }
}

impl ImageCache {
    /// A cache holding at most `max_images` images and `max_bytes` of payload
    /// plus pixels. Zero means the corresponding bound is not enforced.
    pub fn new(max_images: usize, max_bytes: usize) -> Self {
        Self {
            images: VecDeque::new(),
            max_images,
            max_bytes,
            bytes: 0,
            next_sequence: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Bytes held by payloads and decoded pixels together.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.bytes = 0;
    }

    /// Keep `event` at `row`/`column`. A transmission with no display, or one
    /// the renderer cannot draw, is not cached: there is nothing to place.
    ///
    /// Returns the cached image's id when it fits the cache. A re-transmitted
    /// kitty id replaces the old copy, which is what the protocol asks for.
    pub fn insert(&mut self, event: ImageEvent, row: usize, column: usize) -> Option<u32> {
        if !event.display || !event.is_renderable() {
            return None;
        }
        let rgba = event.rgba.clone()?;
        let cost = event.payload.len() + rgba.len();
        if self.max_bytes > 0 && cost > self.max_bytes {
            return None;
        }

        self.remove(event.id);
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.images.push_back(CachedImage {
            id: event.id,
            format: event.format,
            width: event.width,
            height: event.height,
            payload: event.payload,
            rgba,
            row,
            column,
            columns: event.columns,
            rows: event.rows,
            sequence,
        });
        self.bytes += cost;
        self.enforce_bounds();
        Some(event.id)
    }

    /// Drop the placement with this image id, if the cache holds one.
    pub fn remove(&mut self, id: u32) {
        let mut removed = 0usize;
        self.images.retain(|image| {
            if image.id == id {
                removed += image.payload.len() + image.rgba.len();
                false
            } else {
                true
            }
        });
        self.bytes = self.bytes.saturating_sub(removed);
    }

    fn enforce_bounds(&mut self) {
        while self.max_images > 0 && self.images.len() > self.max_images {
            if let Some(image) = self.images.pop_front() {
                self.bytes = self
                    .bytes
                    .saturating_sub(image.payload.len() + image.rgba.len());
            }
        }
        while self.max_bytes > 0 {
            if self.bytes <= self.max_bytes {
                break;
            }
            let Some(image) = self.images.pop_front() else {
                break;
            };
            self.bytes = self
                .bytes
                .saturating_sub(image.payload.len() + image.rgba.len());
        }
    }

    /// Every cached image whose absolute row is inside the viewport, mapped to
    /// viewport rows.
    pub fn visible(
        &self,
        history: usize,
        display_offset: usize,
        screen_lines: usize,
    ) -> Vec<ImagePlacement> {
        let top = history.saturating_sub(display_offset);
        let bottom = top.saturating_add(screen_lines);
        self.images
            .iter()
            .filter(|image| image.row >= top && image.row < bottom)
            .map(|image| ImagePlacement {
                id: image.id,
                format: image.format,
                width: image.width,
                height: image.height,
                row: image.row - top,
                column: image.column,
                columns: image.columns,
                rows: image.rows,
                rgba: Arc::clone(&image.rgba),
                sequence: image.sequence,
            })
            .collect()
    }

    /// The cache in insertion order. Tests and diagnostics read absolute rows;
    /// the renderer uses [`Self::visible`].
    pub fn images(&self) -> &VecDeque<CachedImage> {
        &self.images
    }
}

/// A kitty delete (`a=d`) request, in the terms the cache understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageDelete {
    /// Delete one image and its placement.
    Image(u32),
    /// Delete every placement, keeping transmitted pixels for a later `a=p`.
    AllPlacements,
    /// Delete every image, its placement and its stored transmission.
    All,
}

/// What the image tap found in one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageChunk {
    /// Bytes the VT parser should process.
    Bytes(Vec<u8>),
    /// A decoded image, to be placed at the cursor after the preceding bytes
    /// have been processed.
    Image(Box<ImageEvent>),
    /// A kitty delete, applied to the placements cached for this screen.
    Delete(ImageDelete),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    Apc,
    Dcs,
}

impl BodyKind {
    fn intro(self) -> &'static [u8] {
        match self {
            Self::Apc => b"\x1b_",
            Self::Dcs => b"\x1bP",
        }
    }
}

#[derive(Debug)]
enum TapState {
    Ground,
    /// Saw `ESC`; the next byte decides whether this is an APC/DCS.
    Escape,
    /// Collecting a string body up to `ESC \`.
    Body {
        kind: BodyKind,
        body: Vec<u8>,
        esc: bool,
    },
}

#[derive(Debug)]
struct PendingKitty {
    id: u32,
    control: String,
    payload: String,
}

/// A transmission the tap keeps so a later kitty `a=p` can place it.
#[derive(Debug)]
struct StoredImage {
    event: ImageEvent,
    bytes: usize,
}

/// Splits PTY bytes into pass-through chunks and decoded images.
#[derive(Debug)]
pub struct ImageTap {
    state: TapState,
    next_id: u32,
    pending: Option<PendingKitty>,
    /// Images received with `a=t` or `a=T`, bounded like the placement cache.
    stored: VecDeque<StoredImage>,
    stored_bytes: usize,
    dropped: usize,
}

impl Default for ImageTap {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageTap {
    pub fn new() -> Self {
        Self {
            state: TapState::Ground,
            next_id: 1,
            pending: None,
            stored: VecDeque::new(),
            stored_bytes: 0,
            dropped: 0,
        }
    }

    /// Sequences abandoned as malformed, oversized or undecodable. Observation
    /// is best effort: one bad image never fails the session.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Split `input` into ordered pass-through bytes and images. Bytes that may
    /// begin an image sequence are held until the next call, so a sequence
    /// split across two PTY reads still decodes.
    pub fn tap(&mut self, input: &[u8]) -> Vec<ImageChunk> {
        let mut out = Vec::new();
        for &byte in input {
            self.step(byte, &mut out);
        }
        out
    }

    /// Release bytes held back at the end of a stream. A complete sequence has
    /// already been delivered; an incomplete one is passed through rather than
    /// silently swallowed.
    pub fn flush(&mut self) -> Vec<ImageChunk> {
        let mut out = Vec::new();
        match std::mem::replace(&mut self.state, TapState::Ground) {
            TapState::Ground => {}
            TapState::Escape => out.push(ImageChunk::Bytes(vec![ESC])),
            TapState::Body { kind, body, .. } => {
                let mut bytes = kind.intro().to_vec();
                bytes.extend(body);
                out.push(ImageChunk::Bytes(bytes));
            }
        }
        if self.pending.take().is_some() {
            self.dropped += 1;
        }
        out
    }

    fn step(&mut self, byte: u8, out: &mut Vec<ImageChunk>) {
        match std::mem::replace(&mut self.state, TapState::Ground) {
            TapState::Ground => {
                if byte == ESC {
                    self.state = TapState::Escape;
                } else {
                    push_bytes(out, &[byte]);
                }
            }
            TapState::Escape => match byte {
                b'_' => {
                    self.state = TapState::Body {
                        kind: BodyKind::Apc,
                        body: Vec::new(),
                        esc: false,
                    }
                }
                b'P' => {
                    self.state = TapState::Body {
                        kind: BodyKind::Dcs,
                        body: Vec::new(),
                        esc: false,
                    }
                }
                // `ESC ESC` leaves the first escape alone and keeps looking.
                ESC => {
                    push_bytes(out, &[ESC]);
                    self.state = TapState::Escape;
                }
                _ => {
                    push_bytes(out, &[ESC, byte]);
                    self.state = TapState::Ground;
                }
            },
            TapState::Body {
                kind,
                mut body,
                esc,
            } => {
                if esc {
                    if byte == ST_FINAL {
                        self.finish_body(kind, body, out);
                    } else {
                        // An unescaped ESC inside a string: hand the held bytes
                        // back so the parser sees the stream we could not.
                        let mut bytes = kind.intro().to_vec();
                        bytes.extend(body);
                        bytes.extend([ESC, byte]);
                        out.push(ImageChunk::Bytes(bytes));
                        self.state = TapState::Ground;
                    }
                    return;
                }
                if byte == ESC {
                    // A BEL terminates an APC too, even though the kitty
                    // protocol's own examples use ST.
                    self.state = TapState::Body {
                        kind,
                        body,
                        esc: true,
                    };
                    return;
                }
                if kind == BodyKind::Apc && byte == BEL {
                    self.finish_body(kind, body, out);
                    return;
                }
                body.push(byte);
                if body.len() > MAX_SEQUENCE_BYTES {
                    self.dropped += 1;
                    // A non-image DCS may still matter to the parser; an APC
                    // that grew past the cap is not an image we can render.
                    if kind == BodyKind::Dcs {
                        let mut bytes = kind.intro().to_vec();
                        bytes.extend(body);
                        out.push(ImageChunk::Bytes(bytes));
                    }
                    self.state = TapState::Ground;
                    return;
                }
                self.state = TapState::Body { kind, body, esc };
            }
        }
    }

    fn finish_body(&mut self, kind: BodyKind, body: Vec<u8>, out: &mut Vec<ImageChunk>) {
        match kind {
            BodyKind::Apc => {
                if body.first() == Some(&b'G') {
                    match self.parse_kitty(&body) {
                        KittyOutcome::Image(event) => out.push(ImageChunk::Image(Box::new(event))),
                        KittyOutcome::Delete(delete) => out.push(ImageChunk::Delete(delete)),
                        // A query is deliberately not answered; a chunked
                        // transmission waits for its successor.
                        KittyOutcome::Ignored | KittyOutcome::Pending => {}
                        KittyOutcome::Dropped => self.dropped += 1,
                    }
                } else {
                    let mut bytes = kind.intro().to_vec();
                    bytes.extend(body);
                    bytes.extend(b"\x1b\\");
                    out.push(ImageChunk::Bytes(bytes));
                }
            }
            BodyKind::Dcs => {
                if let Some(data) = sixel_payload(&body) {
                    match decode_sixel(data) {
                        Some((width, height, rgba)) => {
                            let id = self.take_id(None);
                            out.push(ImageChunk::Image(Box::new(ImageEvent {
                                id,
                                format: ImageFormat::Sixel,
                                width,
                                height,
                                payload: Arc::from(data),
                                rgba: Some(Arc::from(rgba)),
                                columns: None,
                                rows: None,
                                display: true,
                            })));
                        }
                        None => self.dropped += 1,
                    }
                } else {
                    let mut bytes = kind.intro().to_vec();
                    bytes.extend(body);
                    bytes.extend(b"\x1b\\");
                    out.push(ImageChunk::Bytes(bytes));
                }
            }
        }
        self.state = TapState::Ground;
    }

    fn take_id(&mut self, wanted: Option<u32>) -> u32 {
        if let Some(id) = wanted.filter(|id| *id != 0) {
            self.next_id = self.next_id.max(id.saturating_add(1));
            return id;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    fn parse_kitty(&mut self, body: &[u8]) -> KittyOutcome {
        let Some(body) = body.strip_prefix(b"G") else {
            return KittyOutcome::Dropped;
        };
        let (control, payload) = match body.iter().position(|&byte| byte == b';') {
            Some(index) => {
                let (Ok(control), Ok(payload)) = (
                    std::str::from_utf8(&body[..index]),
                    std::str::from_utf8(&body[index + 1..]),
                ) else {
                    return KittyOutcome::Dropped;
                };
                (control, payload.to_string())
            }
            None => {
                let Ok(control) = std::str::from_utf8(body) else {
                    return KittyOutcome::Dropped;
                };
                (control, String::new())
            }
        };
        let fields = control_fields(control);

        let initial_action = fields.get(&b'a').and_then(|value| value.chars().next());
        let more = fields.get(&b'm').is_some_and(|value| value == "1");
        let id = fields
            .get(&b'i')
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|id| *id != 0)
            .unwrap_or(0);

        if more {
            match self.pending.as_mut() {
                Some(pending) => pending.payload.push_str(&payload),
                None => {
                    self.pending = Some(PendingKitty {
                        id,
                        control: control.to_string(),
                        payload,
                    });
                }
            }
            return KittyOutcome::Pending;
        }

        let (control, payload) = match self.pending.take() {
            Some(mut pending) => {
                pending.payload.push_str(&payload);
                if pending.id == 0 {
                    pending.id = id;
                }
                (pending.control, pending.payload)
            }
            None => (control.to_string(), payload),
        };
        let fields = control_fields(&control);
        let action =
            initial_action.or_else(|| fields.get(&b'a').and_then(|value| value.chars().next()));
        let requested_id = id.max(
            fields
                .get(&b'i')
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0),
        );

        let columns = fields
            .get(&b'c')
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);
        let rows = fields
            .get(&b'r')
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0);

        match action {
            Some('d') => return self.delete_image(&fields, requested_id),
            // A query asks the terminal to describe its support. This tap
            // decodes pixels; it has no reply channel, so it ignores it.
            Some('q') => return KittyOutcome::Ignored,
            Some('p') => {
                // Display pixels transmitted earlier. The placement geometry
                // may be overridden here; the pixels and format are the
                // transmission's.
                let Some(stored) = self
                    .stored
                    .iter()
                    .find(|stored| stored.event.id == requested_id)
                else {
                    return KittyOutcome::Dropped;
                };
                let mut event = stored.event.clone();
                event.display = true;
                if columns.is_some() {
                    event.columns = columns;
                }
                if rows.is_some() {
                    event.rows = rows;
                }
                return KittyOutcome::Image(event);
            }
            None | Some('t') | Some('T') => {}
            _ => return KittyOutcome::Dropped,
        }

        // Only transmit/display actions carry pixels here.
        let display = action == Some('T');

        let format = match fields
            .get(&b'f')
            .and_then(|value| value.parse::<u32>().ok())
        {
            Some(100) => ImageFormat::KittyPng,
            Some(24) => ImageFormat::KittyRgb,
            Some(32) | None => ImageFormat::KittyRgba,
            _ => return KittyOutcome::Dropped,
        };
        let declared_width = fields
            .get(&b's')
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let declared_height = fields
            .get(&b'v')
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);

        let Ok(payload) = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            payload.as_bytes(),
        ) else {
            return KittyOutcome::Dropped;
        };
        if payload.len() > MAX_SEQUENCE_BYTES {
            return KittyOutcome::Dropped;
        }

        let (width, height, rgba) = match format {
            ImageFormat::KittyPng => {
                if payload.len() < 24 || &payload[..8] != b"\x89PNG\r\n\x1a\n" {
                    return KittyOutcome::Dropped;
                }
                let declared_width =
                    u32::from_be_bytes(payload[16..20].try_into().unwrap_or([0; 4]));
                let declared_height =
                    u32::from_be_bytes(payload[20..24].try_into().unwrap_or([0; 4]));
                if declared_width == 0
                    || declared_height == 0
                    || u64::from(declared_width) * u64::from(declared_height) > MAX_IMAGE_PIXELS
                {
                    return KittyOutcome::Dropped;
                }
                let Ok(decoded) =
                    image::load_from_memory_with_format(&payload, image::ImageFormat::Png)
                else {
                    return KittyOutcome::Dropped;
                };
                let rgba = decoded.into_rgba8();
                let (width, height) = rgba.dimensions();
                (width, height, rgba.into_raw())
            }
            ImageFormat::KittyRgb => {
                if declared_width == 0 || declared_height == 0 {
                    return KittyOutcome::Dropped;
                }
                let expected = declared_width as usize * declared_height as usize * 3;
                if payload.len() < expected {
                    return KittyOutcome::Dropped;
                }
                let mut rgba = Vec::with_capacity(expected / 3 * 4);
                for pixel in payload[..expected].chunks_exact(3) {
                    rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
                (declared_width, declared_height, rgba)
            }
            ImageFormat::KittyRgba => {
                if declared_width == 0 || declared_height == 0 {
                    return KittyOutcome::Dropped;
                }
                let expected = declared_width as usize * declared_height as usize * 4;
                if payload.len() < expected {
                    return KittyOutcome::Dropped;
                }
                (
                    declared_width,
                    declared_height,
                    payload[..expected].to_vec(),
                )
            }
            ImageFormat::Sixel => return KittyOutcome::Dropped,
        };

        let id = self.take_id(Some(requested_id));
        let event = ImageEvent {
            id,
            format,
            width,
            height,
            payload: Arc::from(payload),
            rgba: Some(Arc::from(rgba)),
            columns,
            rows,
            display,
        };
        self.store(&event);
        KittyOutcome::Image(event)
    }

    /// Apply a kitty `a=d` control. The tap removes its stored copy where the
    /// delete applies to the image data; the returned event tells the screen
    /// which placements to drop.
    fn delete_image(&mut self, fields: &HashMap<u8, String>, id: u32) -> KittyOutcome {
        let spec = fields.get(&b'd').and_then(|value| value.chars().next());
        let delete = match spec {
            Some('a') => {
                self.clear_stored();
                ImageDelete::All
            }
            // Every placement goes; the transmitted pixels stay available for
            // a later `a=p`.
            Some('A') => ImageDelete::AllPlacements,
            // Placement-scoped deletes keep the transmission. The cache is
            // keyed by image id, so `I` and `P` resolve to that placement.
            Some('I') | Some('P') => {
                if id == 0 {
                    return KittyOutcome::Ignored;
                }
                ImageDelete::Image(id)
            }
            // Placement ids are not tracked separately; a sequence that names
            // the image as well can still delete that image's placement.
            Some('p') => {
                if id == 0 {
                    return KittyOutcome::Ignored;
                }
                ImageDelete::Image(id)
            }
            Some('i') | None => {
                if id == 0 {
                    self.clear_stored();
                    ImageDelete::All
                } else {
                    self.remove_stored(id);
                    ImageDelete::Image(id)
                }
            }
            Some(_) => return KittyOutcome::Dropped,
        };
        KittyOutcome::Delete(delete)
    }

    fn store(&mut self, event: &ImageEvent) {
        let bytes = event.payload.len() + event.rgba.as_ref().map_or(0, |rgba| rgba.len());
        if bytes > DEFAULT_IMAGE_CACHE_BYTES {
            return;
        }
        if let Some(index) = self
            .stored
            .iter()
            .position(|stored| stored.event.id == event.id)
        {
            if let Some(old) = self.stored.remove(index) {
                self.stored_bytes = self.stored_bytes.saturating_sub(old.bytes);
            }
        }
        self.stored.push_back(StoredImage {
            event: event.clone(),
            bytes,
        });
        self.stored_bytes = self.stored_bytes.saturating_add(bytes);
        while self.stored.len() > DEFAULT_IMAGE_CACHE_IMAGES {
            self.pop_stored();
        }
        while self.stored_bytes > DEFAULT_IMAGE_CACHE_BYTES && self.stored.len() > 1 {
            self.pop_stored();
        }
    }

    fn pop_stored(&mut self) {
        if let Some(stored) = self.stored.pop_front() {
            self.stored_bytes = self.stored_bytes.saturating_sub(stored.bytes);
        }
    }

    fn remove_stored(&mut self, id: u32) {
        if let Some(index) = self.stored.iter().position(|stored| stored.event.id == id) {
            if let Some(stored) = self.stored.remove(index) {
                self.stored_bytes = self.stored_bytes.saturating_sub(stored.bytes);
            }
        }
    }

    fn clear_stored(&mut self) {
        self.stored.clear();
        self.stored_bytes = 0;
    }
}

/// Which of the kitty control fields this tap understood.
enum KittyOutcome {
    Image(ImageEvent),
    Delete(ImageDelete),
    /// A query, deliberately left unanswered.
    Ignored,
    /// A chunked transmission is waiting for its successor.
    Pending,
    /// Malformed or unsupported; counted as dropped.
    Dropped,
}

fn control_fields(control: &str) -> HashMap<u8, String> {
    let mut fields = HashMap::new();
    for pair in control.split(',') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.len() == 1 {
            fields.insert(key.as_bytes()[0], value.to_string());
        }
    }
    fields
}

fn push_bytes(out: &mut Vec<ImageChunk>, bytes: &[u8]) {
    if let Some(ImageChunk::Bytes(existing)) = out.last_mut() {
        existing.extend_from_slice(bytes);
    } else {
        out.push(ImageChunk::Bytes(bytes.to_vec()));
    }
}

/// The sixel data after `q`, when this DCS body is sixel. A DCS body is sixel
/// when everything before its first `q` is a numeric parameter list.
fn sixel_payload(body: &[u8]) -> Option<&[u8]> {
    let q = body.iter().position(|&byte| byte == b'q')?;
    if body[..q]
        .iter()
        .all(|byte| byte.is_ascii_digit() || *byte == b';')
    {
        Some(&body[q + 1..])
    } else {
        None
    }
}

/// The default VT340 16-colour palette, used until a sixel stream defines its
/// own colours.
const DEFAULT_SIXEL_PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

#[derive(Debug, Default)]
struct SixelRaster {
    rows: Vec<Vec<[u8; 4]>>,
    max_x: Option<usize>,
    max_y: Option<usize>,
}

impl SixelRaster {
    fn draw(
        &mut self,
        x: usize,
        y: usize,
        sixel: u8,
        repeat: usize,
        palette: &HashMap<u16, [u8; 4]>,
        current: u16,
    ) -> Option<()> {
        let bits = sixel.wrapping_sub(0x3f);
        let color = *palette.get(&current).unwrap_or(&[0, 0, 0, 255]);
        if x.saturating_add(repeat) > MAX_SIXEL_DIMENSION
            || x.saturating_add(repeat).saturating_mul(y.saturating_add(6)) > MAX_SIXEL_PIXELS
        {
            return None;
        }
        for offset in 0..6 {
            if bits & (1 << offset) == 0 {
                continue;
            }
            let row = y.checked_add(offset)?;
            if row >= MAX_SIXEL_DIMENSION {
                return None;
            }
            while self.rows.len() <= row {
                self.rows.push(Vec::new());
            }
            for x in x..x + repeat {
                let line = &mut self.rows[row];
                if line.len() <= x {
                    line.resize(x + 1, TRANSPARENT);
                }
                line[x] = color;
            }
            self.max_y = Some(self.max_y.map_or(row, |current| current.max(row)));
        }
        if bits != 0 {
            self.max_x = Some(
                self.max_x
                    .map_or(x + repeat - 1, |current: usize| current.max(x + repeat - 1)),
            );
        }
        Some(())
    }
}

/// Decode sixel data into RGBA8. `None` when the stream declared nothing that
/// could be drawn or exceeded the decoder's dimension cap.
///
/// Supported operators: `!` repeat, `#` palette select/define, `$` carriage
/// return, `-` next sixel band, `"` raster attributes and `?` transparent.
pub fn decode_sixel(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut palette: HashMap<u16, [u8; 4]> = DEFAULT_SIXEL_PALETTE
        .iter()
        .enumerate()
        .map(|(index, (r, g, b))| (index as u16, [*r, *g, *b, 255]))
        .collect();
    let mut current: u16 = 0;
    let mut raster = SixelRaster::default();
    let mut x = 0usize;
    let mut y = 0usize;
    let mut index = 0usize;

    while index < data.len() {
        let byte = data[index];
        match byte {
            b'!' => {
                index += 1;
                let (count, next) = parse_number(data, index)?;
                index = next;
                let repeat = (count as usize).clamp(1, MAX_SIXEL_DIMENSION);
                let Some(&sixel) = data.get(index) else {
                    break;
                };
                index += 1;
                if (0x3f..=0x7e).contains(&sixel) {
                    raster.draw(x, y, sixel, repeat, &palette, current)?;
                    x = x.saturating_add(repeat);
                }
            }
            b'#' => {
                index += 1;
                let mut values = Vec::new();
                loop {
                    let (value, next) = parse_number(data, index)?;
                    values.push(value as u16);
                    index = next;
                    if data.get(index) == Some(&b';') {
                        index += 1;
                    } else {
                        break;
                    }
                }
                if let Some(&pc) = values.first() {
                    current = pc;
                    if values.len() >= 5 {
                        let to_byte = |value: u16| ((value.min(100) as u32 * 255) / 100) as u8;
                        palette.insert(
                            pc,
                            [
                                to_byte(values[2]),
                                to_byte(values[3]),
                                to_byte(values[4]),
                                255,
                            ],
                        );
                    }
                }
            }
            b'"' => {
                index += 1;
                let mut values = Vec::new();
                while values.len() < 4 {
                    let (value, next) = parse_number(data, index)?;
                    values.push(value);
                    index = next;
                    if data.get(index) == Some(&b';') {
                        index += 1;
                    } else {
                        break;
                    }
                }
                if let Some(&height) = values.get(3) {
                    if height > MAX_SIXEL_DIMENSION as u32 {
                        return None;
                    }
                }
            }
            b'$' => {
                index += 1;
                x = 0;
            }
            b'-' => {
                index += 1;
                x = 0;
                y = y.saturating_add(6);
                if y >= MAX_SIXEL_DIMENSION {
                    return None;
                }
            }
            0x3f..=0x7e => {
                index += 1;
                raster.draw(x, y, byte, 1, &palette, current)?;
                x = x.saturating_add(1);
            }
            _ => index += 1,
        }
    }

    let (width, height) = match (raster.max_x, raster.max_y) {
        (Some(max_x), Some(max_y)) => (max_x + 1, max_y + 1),
        _ => return None,
    };
    if width == 0 || height == 0 || width > MAX_SIXEL_DIMENSION || height > MAX_SIXEL_DIMENSION {
        return None;
    }
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in raster.rows.iter().take(height) {
        for x in 0..width {
            rgba.extend_from_slice(row.get(x).unwrap_or(&TRANSPARENT));
        }
    }
    Some((width as u32, height as u32, rgba))
}

fn parse_number(data: &[u8], mut index: usize) -> Option<(u32, usize)> {
    let start = index;
    let mut value: u32 = 0;
    while let Some(&byte) = data.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add((byte - b'0') as u32);
        index += 1;
    }
    (index > start).then_some((value, index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{Screen, ScreenConfig, ScreenSize};

    const RED_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

    fn rgba2(pixels: &[[u8; 4]]) -> Vec<u8> {
        pixels.iter().flatten().copied().collect()
    }

    fn raw_event(id: u32, format: ImageFormat, payload: Vec<u8>, rgba: Vec<u8>) -> ImageEvent {
        let pixels = rgba.len() / 4;
        ImageEvent {
            id,
            format,
            width: pixels as u32,
            height: 1,
            payload: Arc::from(payload),
            rgba: Some(Arc::from(rgba)),
            columns: None,
            rows: None,
            display: true,
        }
    }

    #[test]
    fn a_kitty_rgba_sequence_decodes_to_typed_pixels() {
        let pixels = [[255u8, 0, 0, 255], [0, 255, 0, 255]];
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, rgba2(&pixels));
        let mut tap = ImageTap::new();
        let input = format!("\x1b_Ga=T,f=32,s=2,v=1,i=7,c=2,r=1;{encoded}\x1b\\");
        let chunks = tap.tap(input.as_bytes());

        assert_eq!(chunks.len(), 1, "{chunks:?}");
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected an image: {chunks:?}");
        };
        assert_eq!(image.id, 7);
        assert_eq!(image.format, ImageFormat::KittyRgba);
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!((image.columns, image.rows), (Some(2), Some(1)));
        assert!(image.display);
        assert_eq!(&*image.payload, rgba2(&pixels).as_slice());
        assert_eq!(
            image.rgba.as_deref(),
            Some(rgba2(&pixels).as_slice()),
            "RGBA passes through unchanged"
        );
    }

    #[test]
    fn a_kitty_png_sequence_decodes_through_the_png_decoder() {
        let mut tap = ImageTap::new();
        let input = format!("\x1b_Ga=T,f=100,i=9;{RED_PNG_B64}\x1b\\");
        let chunks = tap.tap(input.as_bytes());
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected an image: {chunks:?}");
        };
        assert_eq!(image.format, ImageFormat::KittyPng);
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.rgba.as_deref(), Some([255, 0, 0, 255].as_slice()));
    }

    #[test]
    fn a_chunked_kitty_transmission_is_joined_by_id() {
        let pixels = [[1u8, 2, 3, 255], [4, 5, 6, 255]];
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, rgba2(&pixels));
        let (first, second) = encoded.split_at(encoded.len() / 2);
        let mut tap = ImageTap::new();
        let mut chunks =
            tap.tap(format!("\x1b_Ga=T,f=32,s=2,v=1,i=5,m=1;{first}\x1b\\").as_bytes());
        assert!(chunks.is_empty(), "the first chunk waits for the rest");
        chunks.extend(tap.tap(format!("\x1b_Gm=0,i=5;{second}\x1b\\").as_bytes()));
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected an image: {chunks:?}");
        };
        assert_eq!(image.id, 5);
        assert_eq!(image.rgba.as_deref(), Some(rgba2(&pixels).as_slice()));
    }

    #[test]
    fn a_sixel_sequence_decodes_its_palette_and_sixel_bits() {
        // 3 pixels wide, six rows tall, colour 1 defined as 100% red.
        let mut tap = ImageTap::new();
        let chunks = tap.tap(b"\x1bPq\"1;1;3;6#1;2;100;0;0!3~\x1b\\");
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected an image: {chunks:?}");
        };
        assert_eq!(image.format, ImageFormat::Sixel);
        assert_eq!((image.width, image.height), (3, 6));
        let rgba = image.rgba.as_deref().expect("the sixel decoded");
        assert_eq!(rgba.len(), 3 * 6 * 4);
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn sixel_transparent_pixels_stay_transparent() {
        let (width, height, rgba) = decode_sixel(b"#1;2;0;0;100?@").expect("decoded");
        assert_eq!((width, height), (2, 1));
        assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255]);
    }

    #[test]
    fn foreign_apc_and_dcs_bytes_still_pass_through() {
        let mut tap = ImageTap::new();
        let chunks = tap.tap(b"\x1b[31mred\x1bP>|x\x1b\\\x1b_notkitty\x1b\\");
        let bytes: Vec<u8> = chunks
            .into_iter()
            .flat_map(|chunk| match chunk {
                ImageChunk::Bytes(bytes) => bytes,
                ImageChunk::Image(_) | ImageChunk::Delete(_) => Vec::new(),
            })
            .collect();
        assert_eq!(bytes, b"\x1b[31mred\x1bP>|x\x1b\\\x1b_notkitty\x1b\\");
    }

    #[test]
    fn a_sequence_split_across_reads_decodes_once() {
        let pixels = [[9u8, 8, 7, 255]];
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, rgba2(&pixels));
        let mut tap = ImageTap::new();
        let mut chunks =
            tap.tap(format!("\x1b_Ga=T,f=32,s=1,v=1,i=3;{}", &encoded[..6]).as_bytes());
        assert!(chunks.is_empty());
        chunks.extend(tap.tap(format!("{}\x1b\\", &encoded[6..]).as_bytes()));
        assert!(matches!(chunks.first(), Some(ImageChunk::Image(_))));
    }

    #[test]
    fn the_cache_evicts_the_oldest_image_past_its_count() {
        let mut cache = ImageCache::new(2, 0);
        for id in 1..=3 {
            cache.insert(
                raw_event(id, ImageFormat::KittyRgba, vec![0; 4], vec![0; 4]),
                id as usize,
                0,
            );
        }
        assert_eq!(cache.len(), 2);
        let ids: Vec<u32> = cache.images().iter().map(|image| image.id).collect();
        assert_eq!(ids, vec![2, 3], "the oldest image is evicted");
        assert_eq!(
            cache
                .images()
                .iter()
                .map(|image| image.row)
                .collect::<Vec<_>>(),
            vec![2, 3],
            "the surviving placements keep their rows"
        );
    }

    #[test]
    fn the_cache_evicts_past_its_byte_budget() {
        // Each image costs 4 payload bytes + 4 pixel bytes.
        let mut cache = ImageCache::new(0, 16);
        for id in 1..=4 {
            cache.insert(
                raw_event(id, ImageFormat::KittyRgba, vec![0; 4], vec![0; 4]),
                id as usize,
                0,
            );
        }
        assert!(cache.bytes() <= 16);
        let ids: Vec<u32> = cache.images().iter().map(|image| image.id).collect();
        assert_eq!(ids, vec![3, 4]);
    }

    #[test]
    fn a_transmission_without_display_is_not_placed() {
        let mut event = raw_event(1, ImageFormat::KittyRgba, vec![0; 4], vec![0; 4]);
        event.display = false;
        let mut cache = ImageCache::default();
        assert_eq!(cache.insert(event, 0, 0), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn a_kitty_placement_displays_an_earlier_transmission() {
        let mut tap = ImageTap::new();
        let chunks = tap.tap(b"\x1b_Ga=T,f=32,s=1,v=1,i=9;/wAA/w==\x1b\\");
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected the transmission: {chunks:?}");
        };
        assert!(image.display);

        let chunks = tap.tap(b"\x1b_Ga=p,i=9,x=3,y=2\x1b\\");
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected the placement: {chunks:?}");
        };
        assert!(image.display);
        assert_eq!(image.id, 9);
        assert_eq!(image.rgba.as_deref(), Some([255, 0, 0, 255].as_slice()));

        // Placement geometry can override what the transmission carried.
        let chunks = tap.tap(b"\x1b_Ga=p,i=9,c=2,r=1\x1b\\");
        let ImageChunk::Image(image) = &chunks[0] else {
            panic!("expected the second placement: {chunks:?}");
        };
        assert_eq!((image.columns, image.rows), (Some(2), Some(1)));
    }

    #[test]
    fn a_kitty_delete_removes_the_transmission_and_its_placement() {
        let mut tap = ImageTap::new();
        tap.tap(b"\x1b_Ga=T,f=32,s=1,v=1,i=9;/wAA/w==\x1b\\");
        assert_eq!(
            tap.tap(b"\x1b_Ga=d,i=9\x1b\\"),
            vec![ImageChunk::Delete(ImageDelete::Image(9))]
        );
        assert!(
            tap.tap(b"\x1b_Ga=p,i=9\x1b\\").is_empty(),
            "a deleted image cannot be placed"
        );
    }

    #[test]
    fn a_kitty_delete_all_clears_the_transmissions() {
        let mut tap = ImageTap::new();
        tap.tap(b"\x1b_Ga=t,f=32,s=1,v=1,i=9;/wAA/w==\x1b\\");
        assert_eq!(
            tap.tap(b"\x1b_Ga=d,d=a\x1b\\"),
            vec![ImageChunk::Delete(ImageDelete::All)]
        );
        assert!(tap.tap(b"\x1b_Ga=p,i=9\x1b\\").is_empty());
    }

    #[test]
    fn a_kitty_delete_reaches_the_screens_placement_cache() {
        let mut screen = Screen::new(ScreenSize::new(20, 4), ScreenConfig::scrolling(0));
        screen.feed(b"\x1b_Ga=T,f=32,s=1,v=1,i=9;/wAA/w==\x1b\\");
        assert_eq!(screen.images().len(), 1);
        screen.feed(b"\x1b_Ga=d,i=9\x1b\\");
        assert!(screen.images().is_empty());
    }

    #[test]
    fn a_transmitted_image_is_placed_only_when_a_p_asks_for_it() {
        let mut screen = Screen::new(ScreenSize::new(20, 4), ScreenConfig::scrolling(0));
        screen.feed(b"\x1b_Ga=t,f=32,s=1,v=1,i=9;/wAA/w==\x1b\\");
        assert!(
            screen.images().is_empty(),
            "a transmission alone draws nothing"
        );
        screen.feed(b"\x1b_Ga=p,i=9,c=2,r=1\x1b\\");
        let images = screen.images();
        assert_eq!(images.len(), 1);
        assert_eq!((images[0].columns, images[0].rows), (Some(2), Some(1)));
    }

    #[test]
    fn a_kitty_placement_sits_on_the_cursor_cell_and_scrolls_with_content() {
        let pixels = [[1u8, 2, 3, 255]];
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, rgba2(&pixels));
        let mut screen = Screen::new(ScreenSize::new(20, 4), ScreenConfig::scrolling(8));
        screen.feed(b"ab");
        screen.feed(format!("\x1b_Ga=T,f=32,s=1,v=1,i=4;{encoded}\x1b\\").as_bytes());

        let images = screen.images();
        assert_eq!(images.len(), 1);
        assert_eq!((images[0].row, images[0].column), (0, 2));

        // Four newlines scroll the image into history; the absolute row stays
        // the same, so it is still above the viewport until the user scrolls.
        screen.feed(b"\r\n\r\n\r\n\r\n");
        assert_eq!(screen.history_size(), 1);
        assert!(screen.images().is_empty(), "the row is above the viewport");
        screen.scroll_display(1);
        let images = screen.images();
        assert_eq!(images.len(), 1);
        assert_eq!((images[0].row, images[0].column), (0, 2));
        assert_eq!(
            screen.image_cache().images()[0].row,
            0,
            "the placement keeps its absolute content row"
        );
    }

    #[test]
    fn a_reset_clears_the_pane_image_cache() {
        let mut screen = Screen::new(ScreenSize::new(10, 2), ScreenConfig::scrolling(0));
        screen.feed(b"\x1b_Ga=T,f=32,s=1,v=1,i=4;AQIDBA==\x1b\\");
        assert_eq!(screen.images().len(), 1);
        screen.reset();
        assert!(screen.images().is_empty());
    }
}
