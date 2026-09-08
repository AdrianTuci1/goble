//! Screen capture + control transport abstractions.
//!
//! This crate is the `types` layer of a `types <- protocol <- runtime` split
//! for screen I/O. It owns the model-shaped [`ScreenFrame`], the object-safe
//! [`ScreenCapturer`] / [`ScreenController`] seams a runtime drives, a
//! [`ScreenRegistry`] to hold them, and [`MockCapturer`] / [`MockController`]
//! for tests and offline demos. There is no native capture/control backend and
//! no dependency on `goble-core` or `app/`.
//!
//! Real platform backends (X11, macOS, Windows) are gated behind the `platform`
//! feature (off by default), so the crate builds everywhere with its in-crate
//! mocks.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single captured screen image.
///
/// `data` is an RGBA8 buffer of `width * height * 4` bytes. Whether a frame
/// comes from a real platform grab or a recording is a provider concern, not
/// this layer's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenFrame {
    /// Identifier of the screen/display the frame came from.
    pub source: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// When the frame was captured.
    pub timestamp: DateTime<Utc>,
    /// RGBA8 pixel data (`width * height * 4` bytes).
    pub data: Vec<u8>,
}

impl ScreenFrame {
    /// A new frame captured at the current time.
    pub fn new(source: impl Into<String>, width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            source: source.into(),
            width,
            height,
            timestamp: Utc::now(),
            data,
        }
    }

    /// A fully black (zeroed RGBA8) frame of the given dimensions.
    pub fn blank(source: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            source: source.into(),
            width,
            height,
            timestamp: Utc::now(),
            data: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    /// Number of bytes in the pixel buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the pixel buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Errors surfaced by the screen capture/control layer.
#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("no capturer registered for source '{0}'")]
    NoCapturer(String),
    #[error("no controller registered for source '{0}'")]
    NoController(String),
    #[error("platform backend for '{0}' is not yet implemented")]
    UnsupportedPlatform(String),
}

/// Object-safe screen capture seam. Implementors are held as
/// `Arc<dyn ScreenCapturer>` and a runtime never sees a concrete backend.
pub trait ScreenCapturer: Send + Sync {
    /// The source identifier this capturer reads from.
    fn source(&self) -> &str;
    /// Capture the current frame.
    fn capture(&self) -> Result<ScreenFrame>;
}

/// Object-safe screen control seam: pointer and keyboard input to a screen.
pub trait ScreenController: Send + Sync {
    /// The source identifier this controller drives.
    fn source(&self) -> &str;
    /// Click at pixel coordinates `(x, y)`.
    fn click(&self, x: u32, y: u32) -> Result<()>;
    /// Type `text` into the focused field.
    fn type_text(&self, text: &str) -> Result<()>;
    /// Scroll by `dx` horizontal and `dy` vertical ticks.
    fn scroll(&self, dx: i32, dy: i32) -> Result<()>;
}

/// A cloneable, thread-safe registry of capturers and controllers keyed by source.
#[derive(Clone, Default)]
pub struct ScreenRegistry {
    capturers: Arc<RwLock<HashMap<String, Arc<dyn ScreenCapturer>>>>,
    controllers: Arc<RwLock<HashMap<String, Arc<dyn ScreenController>>>>,
}

impl ScreenRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a capturer under its [`ScreenCapturer::source`].
    pub fn register_capturer(&self, capturer: Arc<dyn ScreenCapturer>) {
        self.capturers
            .write()
            .unwrap()
            .insert(capturer.source().to_string(), capturer);
    }

    /// Register a controller under its [`ScreenController::source`].
    pub fn register_controller(&self, controller: Arc<dyn ScreenController>) {
        self.controllers
            .write()
            .unwrap()
            .insert(controller.source().to_string(), controller);
    }

    /// Resolve a capturer by source.
    pub fn capturer(&self, source: &str) -> Option<Arc<dyn ScreenCapturer>> {
        self.capturers.read().unwrap().get(source).cloned()
    }

    /// Resolve a controller by source.
    pub fn controller(&self, source: &str) -> Option<Arc<dyn ScreenController>> {
        self.controllers.read().unwrap().get(source).cloned()
    }

    /// Every registered capturer source.
    pub fn capturer_sources(&self) -> Vec<String> {
        self.capturers.read().unwrap().keys().cloned().collect()
    }

    /// Every registered controller source.
    pub fn controller_sources(&self) -> Vec<String> {
        self.controllers.read().unwrap().keys().cloned().collect()
    }

    /// Capture a frame from `source`.
    pub fn capture(&self, source: &str) -> Result<ScreenFrame> {
        self.capturer(source)
            .ok_or_else(|| ScreenError::NoCapturer(source.to_string()))?
            .capture()
    }

    /// Click at `(x, y)` on `source`.
    pub fn click(&self, source: &str, x: u32, y: u32) -> Result<()> {
        self.controller(source)
            .ok_or_else(|| ScreenError::NoController(source.to_string()))?
            .click(x, y)
    }

    /// Type `text` into `source`.
    pub fn type_text(&self, source: &str, text: &str) -> Result<()> {
        self.controller(source)
            .ok_or_else(|| ScreenError::NoController(source.to_string()))?
            .type_text(text)
    }

    /// Scroll on `source`.
    pub fn scroll(&self, source: &str, dx: i32, dy: i32) -> Result<()> {
        self.controller(source)
            .ok_or_else(|| ScreenError::NoController(source.to_string()))?
            .scroll(dx, dy)
    }
}

/// A configurable [`ScreenCapturer`] for tests and demos: cycles through a
/// configured sequence of frames, falling back to a blank frame when empty.
pub struct MockCapturer {
    source: String,
    frames: Arc<Mutex<VecDeque<ScreenFrame>>>,
}

impl MockCapturer {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            frames: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Add a frame to the queue `capture` cycles through.
    pub fn with_frame(self, frame: ScreenFrame) -> Self {
        self.frames.lock().unwrap().push_back(frame);
        self
    }

    /// Add a blank frame of the given dimensions to the queue.
    pub fn with_blank(self, width: u32, height: u32) -> Self {
        let source = self.source.clone();
        self.with_frame(ScreenFrame::blank(source, width, height))
    }
}

impl ScreenCapturer for MockCapturer {
    fn source(&self) -> &str {
        &self.source
    }

    fn capture(&self) -> Result<ScreenFrame> {
        let mut frames = self.frames.lock().unwrap();
        match frames.pop_front() {
            Some(frame) => {
                frames.push_back(frame.clone());
                Ok(frame)
            }
            None => Ok(ScreenFrame::blank(self.source.clone(), 0, 0)),
        }
    }
}

/// A recorded screen-control action, for inspecting a [`MockController`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockAction {
    Click { x: u32, y: u32 },
    TypeText { text: String },
    Scroll { dx: i32, dy: i32 },
}

/// A [`ScreenController`] for tests and demos that records every action it is
/// asked to perform instead of driving a real screen.
pub struct MockController {
    source: String,
    actions: Arc<Mutex<Vec<MockAction>>>,
}

impl MockController {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            actions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A clone of every action recorded so far, in order.
    pub fn actions(&self) -> Vec<MockAction> {
        self.actions.lock().unwrap().clone()
    }
}

impl ScreenController for MockController {
    fn source(&self) -> &str {
        &self.source
    }

    fn click(&self, x: u32, y: u32) -> Result<()> {
        self.actions
            .lock()
            .unwrap()
            .push(MockAction::Click { x, y });
        Ok(())
    }

    fn type_text(&self, text: &str) -> Result<()> {
        self.actions
            .lock()
            .unwrap()
            .push(MockAction::TypeText { text: text.into() });
        Ok(())
    }

    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        self.actions
            .lock()
            .unwrap()
            .push(MockAction::Scroll { dx, dy });
        Ok(())
    }
}

#[cfg(feature = "platform")]
pub mod backends {
    //! Placeholder for real platform backends (X11, macOS, Windows).
    //!
    //! Gated behind the `platform` feature so the crate builds everywhere with
    //! the in-crate mocks. Linking a native backend is future work; until then
    //! [`PlatformBackend::connect`] fails with [`ScreenError::UnsupportedPlatform`].

    use crate::{ScreenCapturer, ScreenController};
    use anyhow::Result;

    /// A connected platform screen backend: a capturer + controller pair.
    pub struct PlatformBackend {
        _source: String,
        _capturer: Box<dyn ScreenCapturer>,
        _controller: Box<dyn ScreenController>,
    }

    impl PlatformBackend {
        /// Connect to the native platform screen. Currently unavailable: no
        /// backend is linked, so this always fails.
        pub fn connect(_source: &str) -> Result<Self> {
            Err(crate::ScreenError::UnsupportedPlatform(_source.to_string()).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(source: &str, w: u32, h: u32, fill: u8) -> ScreenFrame {
        ScreenFrame::new(source, w, h, vec![fill; (w as usize) * (h as usize) * 4])
    }

    #[test]
    fn screen_frame_roundtrip() {
        let f = frame("display:0", 2, 2, 7);
        let json = serde_json::to_string(&f).unwrap();
        let decoded: ScreenFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, f);
        assert_eq!(decoded.source, "display:0");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
    }

    #[test]
    fn screen_frame_helpers() {
        let f = ScreenFrame::blank("d", 4, 3);
        assert_eq!(f.width, 4);
        assert_eq!(f.height, 3);
        assert_eq!(f.len(), 48);
        assert_eq!(f.data, vec![0u8; 48]);
        assert!(!f.is_empty());
        assert!(ScreenFrame::blank("d", 0, 0).is_empty());
    }

    #[test]
    fn mock_capturer_cycles_frames() {
        let cap = MockCapturer::new("display:0")
            .with_frame(frame("display:0", 1, 1, 1))
            .with_frame(frame("display:0", 1, 1, 2));
        assert_eq!(cap.capture().unwrap().data[0], 1);
        assert_eq!(cap.capture().unwrap().data[0], 2);
        // Cycles back to the first frame.
        assert_eq!(cap.capture().unwrap().data[0], 1);
    }

    #[test]
    fn mock_capturer_falls_back_to_blank() {
        let cap = MockCapturer::new("empty");
        let f = cap.capture().unwrap();
        assert!(f.is_empty());
    }

    #[test]
    fn mock_controller_records_actions() {
        let ctl = MockController::new("display:0");
        ctl.click(10, 20).unwrap();
        ctl.type_text("hi").unwrap();
        ctl.scroll(-1, 2).unwrap();
        assert_eq!(
            ctl.actions(),
            vec![
                MockAction::Click { x: 10, y: 20 },
                MockAction::TypeText { text: "hi".into() },
                MockAction::Scroll { dx: -1, dy: 2 },
            ]
        );
    }

    #[test]
    fn registry_captures_and_controls() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(
            MockCapturer::new("display:0").with_frame(frame("display:0", 1, 1, 9)),
        ));
        let ctl = Arc::new(MockController::new("display:0"));
        reg.register_controller(ctl.clone());

        let f = reg.capture("display:0").unwrap();
        assert_eq!(f.data[0], 9);
        reg.click("display:0", 1, 2).unwrap();
        reg.type_text("display:0", "x").unwrap();
        reg.scroll("display:0", 0, -1).unwrap();

        assert_eq!(reg.capturer_sources(), vec!["display:0".to_string()]);
        assert_eq!(reg.controller_sources(), vec!["display:0".to_string()]);
        assert_eq!(
            ctl.actions(),
            vec![
                MockAction::Click { x: 1, y: 2 },
                MockAction::TypeText { text: "x".into() },
                MockAction::Scroll { dx: 0, dy: -1 },
            ]
        );
    }

    #[test]
    fn registry_missing_source_errors() {
        let reg = ScreenRegistry::new();
        let err = reg.capture("none").unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ScreenError>(),
            Some(ScreenError::NoCapturer(s)) if s == "none"
        ));

        let err = reg.click("none", 0, 0).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ScreenError>(),
            Some(ScreenError::NoController(s)) if s == "none"
        ));
    }

    #[test]
    fn traits_are_object_safe() {
        let cap: Arc<dyn ScreenCapturer> = Arc::new(MockCapturer::new("a"));
        let ctl: Arc<dyn ScreenController> = Arc::new(MockController::new("a"));
        assert_eq!(cap.source(), "a");
        assert_eq!(ctl.source(), "a");
        cap.capture().unwrap();
        ctl.type_text("q").unwrap();
    }

    #[cfg(feature = "platform")]
    #[test]
    fn platform_backend_is_not_yet_implemented() {
        let err = backends::PlatformBackend::connect("x11").unwrap_err();
        assert!(err.downcast_ref::<ScreenError>().is_some());
    }
}
