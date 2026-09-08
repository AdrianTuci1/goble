//! Real local screen capture+control adapter (the `runtime` layer of the
//! `types <- protocol <- runtime` split for screen I/O).
//!
//! This crate implements [`ScreenCapturer`] / [`ScreenController`] for a real
//! local source. On macOS [`LocalAdapter::connect`] yields a capturer whose
//! `capture()` shells out to `screencapture` (or `screencapture -x -t png`)
//! and decodes the resulting PNG into an RGBA8 [`ScreenFrame`] with the
//! workspace `image` crate; on every other platform the adapter yields a
//! synthetic capturer/controller that returns stable blank frames and no-ops
//! input, so the crate — and any [`ScreenRegistry`] that hosts it — always has a
//! live, reachable pair.
//!
//! It depends on `goble-screen-core` (the `types` layer) and touches neither
//! `app/` nor `goble-core`.

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use goble_screen_core::{ScreenCapturer, ScreenController, ScreenFrame, ScreenRegistry};

/// Source identifier for the local/main display and its input device.
pub const LOCAL_SOURCE: &str = "local";

/// A connected local screen adapter: the capturer reads the local display, the
/// controller drives local pointer/keyboard input.
pub struct LocalAdapter {
    /// Captures the local display.
    pub capturer: Arc<dyn ScreenCapturer>,
    /// Drives local pointer/keyboard input.
    pub controller: Arc<dyn ScreenController>,
}

impl LocalAdapter {
    /// Connect to the local screen source `source`.
    ///
    /// On macOS this returns the real `screencapture`-backed capturer; on any
    /// other platform it returns a synthetic adapter so a registry is always
    /// populated. No capture happens here — only [`ScreenCapturer::capture`]
    /// runs the platform grab (which may require screen-recording permission).
    pub fn connect(source: &str) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                capturer: Arc::new(LocalCapturer::new(source)),
                controller: Arc::new(LocalController::new(source)),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self {
                capturer: Arc::new(SyntheticCapturer::new(source, 320, 240)),
                controller: Arc::new(SyntheticController::new(source)),
            })
        }
    }

    /// Register both the capturer and the controller into `registry`.
    pub fn register(&self, registry: &ScreenRegistry) {
        registry.register_capturer(Arc::clone(&self.capturer));
        registry.register_controller(Arc::clone(&self.controller));
    }

    /// Connect to `source` and register both halves, returning the adapter.
    pub fn connect_and_register(source: &str, registry: &ScreenRegistry) -> Result<Self> {
        let adapter = Self::connect(source)?;
        adapter.register(registry);
        Ok(adapter)
    }
}

/// A [`ScreenCapturer`] backed by the macOS `screencapture` utility.
///
/// Only compiled on `target_os = "macos"`.
#[cfg(target_os = "macos")]
pub struct LocalCapturer {
    source: String,
}

#[cfg(target_os = "macos")]
impl LocalCapturer {
    /// A new capturer for `source`.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

#[cfg(target_os = "macos")]
impl ScreenCapturer for LocalCapturer {
    fn source(&self) -> &str {
        &self.source
    }

    fn capture(&self) -> Result<ScreenFrame> {
        capture_macos(&self.source)
    }
}

/// A [`ScreenController`] that drives local pointer/keyboard input.
///
/// Input is best-effort: `cliclick` (pointer/scroll) and AppleScript System
/// Events (typing) are invoked when the tools are present, and any failure is
/// swallowed so a missing tool degrades to a no-op rather than an error. Only
/// compiled on `target_os = "macos"`.
#[cfg(target_os = "macos")]
pub struct LocalController {
    source: String,
}

#[cfg(target_os = "macos")]
impl LocalController {
    /// A new controller for `source`.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

#[cfg(target_os = "macos")]
impl ScreenController for LocalController {
    fn source(&self) -> &str {
        &self.source
    }

    fn click(&self, x: u32, y: u32) -> Result<()> {
        let _ = Command::new("cliclick")
            .args(["c:"])
            .arg(format!("{x},{y}"))
            .status();
        Ok(())
    }

    fn type_text(&self, text: &str) -> Result<()> {
        let script = format!(
            "tell application \"System Events\" to keystroke {}",
            apple_script_string(text)
        );
        let _ = Command::new("osascript").args(["-e", &script]).status();
        Ok(())
    }

    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        let _ = Command::new("cliclick")
            .args(["w:"])
            .arg(format!("{dx},{dy}"))
            .status();
        Ok(())
    }
}

/// Quote `s` as a double-quoted AppleScript string literal.
#[cfg(target_os = "macos")]
fn apple_script_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Capture the full screen on macOS by running `screencapture` and decoding the
/// PNG it writes into an RGBA8 [`ScreenFrame`].
#[cfg(target_os = "macos")]
fn capture_macos(source: &str) -> Result<ScreenFrame> {
    // Thread-safe unique filename so concurrent captures never collide.
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "goble-screen-{}-{}.png",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let output = Command::new("screencapture")
        .args(["-x", "-t", "png"])
        .arg(&path)
        .output()
        .context("failed to spawn `screencapture`")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&path);
        bail!(
            "`screencapture` exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let bytes = std::fs::read(&path).context("failed to read `screencapture` PNG")?;
    let _ = std::fs::remove_file(&path);
    decode_png(source, &bytes)
}

/// Decode an in-memory PNG into an RGBA8 [`ScreenFrame`].
#[cfg(target_os = "macos")]
fn decode_png(source: &str, bytes: &[u8]) -> Result<ScreenFrame> {
    let img = image::load_from_memory(bytes).context("failed to decode PNG")?;
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    let data = rgba.into_raw();
    Ok(ScreenFrame::new(source, width, height, data))
}

/// A [`ScreenCapturer`] that always returns the same configured frame. Used as
/// the local adapter on platforms without a real capture path, so a hosting
/// [`ScreenRegistry`] still holds a reachable capturer.
#[derive(Clone)]
pub struct SyntheticCapturer {
    source: String,
    frame: ScreenFrame,
}

impl SyntheticCapturer {
    /// A new synthetic capturer for `source` returning a blank frame of
    /// `width` x `height`.
    pub fn new(source: impl Into<String>, width: u32, height: u32) -> Self {
        let source = source.into();
        let frame = ScreenFrame::blank(source.clone(), width, height);
        Self {
            source,
            frame,
        }
    }

    /// Override the frame the capturer returns, keeping the source intact.
    pub fn with_frame(mut self, frame: ScreenFrame) -> Self {
        self.frame = frame;
        self
    }
}

impl ScreenCapturer for SyntheticCapturer {
    fn source(&self) -> &str {
        &self.source
    }

    fn capture(&self) -> Result<ScreenFrame> {
        Ok(self.frame.clone())
    }
}

/// A [`ScreenController`] that accepts input but does nothing with it. Used as
/// the local controller on platforms without a real input path.
pub struct SyntheticController {
    source: String,
}

impl SyntheticController {
    /// A new synthetic controller for `source`.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

impl ScreenController for SyntheticController {
    fn source(&self) -> &str {
        &self.source
    }

    fn click(&self, _x: u32, _y: u32) -> Result<()> {
        Ok(())
    }

    fn type_text(&self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn scroll(&self, _dx: i32, _dy: i32) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_capturer_returns_blank_frame() {
        let cap = SyntheticCapturer::new(LOCAL_SOURCE, 320, 240);
        assert_eq!(cap.source(), LOCAL_SOURCE);
        let f = cap.capture().unwrap();
        assert_eq!(f.width, 320);
        assert_eq!(f.height, 240);
        assert_eq!(f.len(), 320 * 240 * 4);
        assert_eq!(f.data, vec![0u8; 320 * 240 * 4]);
    }

    #[test]
    fn synthetic_capturer_can_override_frame() {
        let cap = SyntheticCapturer::new(LOCAL_SOURCE, 1, 1)
            .with_frame(ScreenFrame::new(LOCAL_SOURCE, 2, 2, vec![7u8; 16]));
        let f = cap.capture().unwrap();
        assert_eq!(f.width, 2);
        assert_eq!(f.height, 2);
        assert_eq!(f.data, vec![7u8; 16]);
    }

    #[test]
    fn synthetic_controller_accepts_input_as_noop() {
        let ctl = SyntheticController::new(LOCAL_SOURCE);
        assert_eq!(ctl.source(), LOCAL_SOURCE);
        ctl.click(10, 20).unwrap();
        ctl.type_text("hi").unwrap();
        ctl.scroll(-1, 2).unwrap();
    }

    #[test]
    fn factory_registers_capturer_and_controller() {
        let registry = ScreenRegistry::new();
        let adapter = LocalAdapter::connect(LOCAL_SOURCE).unwrap();
        adapter.register(&registry);
        assert!(registry.capturer(LOCAL_SOURCE).is_some());
        assert!(registry.controller(LOCAL_SOURCE).is_some());
        assert_eq!(registry.capturer_sources(), vec![LOCAL_SOURCE.to_string()]);
        assert_eq!(registry.controller_sources(), vec![LOCAL_SOURCE.to_string()]);
    }

    #[test]
    fn connect_and_register_populates_registry() {
        let registry = ScreenRegistry::new();
        LocalAdapter::connect_and_register(LOCAL_SOURCE, &registry).unwrap();
        assert_eq!(registry.capturer_sources(), vec![LOCAL_SOURCE.to_string()]);
        assert_eq!(registry.controller_sources(), vec![LOCAL_SOURCE.to_string()]);
    }

    /// The PNG-decode path is macOS-specific and independent of
    /// screen-recording permission, so it round-trips an in-memory PNG.
    #[cfg(target_os = "macos")]
    #[test]
    fn decode_png_builds_rgba8_frame() {
        let rgba = image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(
                &mut std::io::Cursor::new(&mut encoded),
                image::ImageFormat::Png,
            )
            .unwrap();
        let frame = decode_png(LOCAL_SOURCE, &encoded).unwrap();
        assert_eq!(frame.source, LOCAL_SOURCE);
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
