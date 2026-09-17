//! Screen capture + control transport abstractions.
//!
//! This crate is the `types` layer of a `types <- protocol <- runtime` split
//! for screen I/O. It owns the model-shaped [`ScreenFrame`], the object-safe
//! [`ScreenCapturer`] / [`ScreenController`] seams a runtime drives, a
//! [`ScreenRegistry`] to hold them, and [`MockCapturer`] / [`MockController`]
//! for tests and offline demos. There is no native capture/control backend and
//! no dependency on `goble-core` or `app/`.
//!
//! The registry also owns the rule that a source has **one writer at a time**:
//! capture reads a capturer, input resolves a controller, and a source with a
//! capturer and no controller is view-only. Control is handed over explicitly
//! through [`ScreenRegistry::take_control`] / [`ScreenRegistry::release_control`],
//! so a desktop two sides can reach (a remote desktop the agent asked for and
//! the user is watching) is never drivable by both.
//!
//! Who may take it from whom is the recorded takeover rule, and it lives here
//! rather than at a call site: the user may take the desktop back from the agent
//! at any time, and the agent may never take it from the user — its take stays
//! refused with [`ScreenError::ControlHeld`] while the user drives.
//!
//! A source is closed with [`ScreenRegistry::unregister_source`], which drops
//! its capturer and its controller together: a closed desktop has no frame and
//! no writer left, rather than a last frame nothing updates.
//!
//! Capture also refuses a source whose client is gone: [`ClientLiveness`] is
//! the shared handle a remote source's capturer consults, so the frame a dead
//! client left behind is reported ([`ScreenError::ClientGone`]) instead of
//! being served as the live desktop.
//!
//! Real platform backends (X11, macOS, Windows) are gated behind the `platform`
//! feature (off by default), so the crate builds everywhere with its in-crate
//! mocks.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Who is driving a source's input.
///
/// Input reaches one holder at a time. A desktop nobody has taken is view-only,
/// and a take while the *other* holder is driving is refused: that refusal is
/// how the two sides arbitrate, and how the agent asks for a desktop the user
/// is driving back. The one take that is never refused is the user's: the user
/// may take the desktop back from the agent at any time, while the agent may
/// never take it from the user ([`ScreenRegistry::take_control`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlHolder {
    /// The person at the machine: their own desktop, or an attended remote one.
    User,
    /// The agent's computer-use path.
    Agent,
}

impl std::fmt::Display for ControlHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => f.write_str("the user"),
            Self::Agent => f.write_str("the agent"),
        }
    }
}

/// Errors surfaced by the screen capture/control layer.
#[derive(Debug, thiserror::Error)]
pub enum ScreenError {
    #[error("no capturer registered for source '{0}'")]
    NoCapturer(String),
    #[error("no controller registered for source '{0}'")]
    NoController(String),
    #[error(
        "source '{source_id}' is driven by {holder}: {asked} cannot drive it until that control \
         is released"
    )]
    ControlHeld {
        /// The source whose input is already held. Not named `source`: thiserror
        /// reads a field of that name as the error's own cause.
        source_id: String,
        /// The holder driving it.
        holder: ControlHolder,
        /// The holder that was refused.
        asked: ControlHolder,
    },
    #[error("platform backend for '{0}' is not yet implemented")]
    UnsupportedPlatform(String),
    #[error(
        "the client behind source '{0}' is gone: the desktop ended, so no frame is served after it"
    )]
    ClientGone(String),
}

/// Whether the client behind a source is still there.
///
/// A local source always is: its capturer reads this machine. A remote one's
/// client can go — its session fails or ends — and the frame it left behind
/// must not be served as if the desktop were. The handle is shared, so the side
/// that holds the desktop sees the client's own transition without polling the
/// client, and the capturer consults the same handle before it hands out a
/// frame ([`ScreenError::ClientGone`]).
#[derive(Clone, Debug)]
pub struct ClientLiveness {
    alive: Arc<AtomicBool>,
}

impl ClientLiveness {
    /// A liveness that starts alive: nothing has gone wrong yet.
    pub fn new() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Whether the client is still there.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Record that the client is gone. Called by the path that owns the
    /// client's session when it fails or ends; every clone sees it at once.
    pub fn mark_gone(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl Default for ClientLiveness {
    fn default() -> Self {
        Self::new()
    }
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

/// A source's driver: the controller registered under it and who holds it.
#[derive(Clone)]
struct Driver {
    controller: Arc<dyn ScreenController>,
    holder: ControlHolder,
}

/// A cloneable, thread-safe registry of capturers and controllers keyed by source.
///
/// A source is view-only while it has no controller: [`ScreenRegistry::capture`]
/// resolves the capturer and works, every input method resolves a controller and
/// fails. Control is taken and released explicitly, one holder at a time.
#[derive(Clone, Default)]
pub struct ScreenRegistry {
    capturers: Arc<RwLock<HashMap<String, Arc<dyn ScreenCapturer>>>>,
    controllers: Arc<RwLock<HashMap<String, Driver>>>,
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

    /// Register a controller under its [`ScreenController::source`], held by
    /// [`ControlHolder::User`].
    ///
    /// This is the **own-desktop** path — one machine, one person at it, nobody
    /// to arbitrate with, which is why `goble-screen-adapter`'s local source
    /// registers both halves here. It is not the arbitrated take: a desktop two
    /// sides can reach (a remote one) takes its controller through
    /// [`Self::take_control`], so the sides never both hold it.
    pub fn register_controller(&self, controller: Arc<dyn ScreenController>) {
        self.controllers.write().unwrap().insert(
            controller.source().to_string(),
            Driver {
                controller,
                holder: ControlHolder::User,
            },
        );
    }

    /// Take `source`'s input for `holder`, registering `controller` under it.
    ///
    /// One writer at a time, with one asymmetry — the recorded takeover rule:
    /// **the user may take the desktop back from the agent at any time, and the
    /// agent may never take it from the user.** A [`ControlHolder::User`] take
    /// replaces an [`ControlHolder::Agent`] holder; an [`ControlHolder::Agent`]
    /// take while the user drives fails with [`ScreenError::ControlHeld`] and
    /// `controller` is not registered. That refusal is also how a side asks for
    /// the desktop: the agent's take while the user drives is its request for
    /// the desktop back, and retrying it once the user releases is what grants
    /// it. (rakazo answers the same request with HTTP 409 and records it on the
    /// run as `waiting_takeover`.) The holder already driving may take again,
    /// replacing its controller — a reconnect.
    pub fn take_control(
        &self,
        source: &str,
        holder: ControlHolder,
        controller: Arc<dyn ScreenController>,
    ) -> Result<()> {
        let mut controllers = self.controllers.write().unwrap();
        if let Some(driver) = controllers.get(source) {
            // The one take that replaces another holder is the user's: the
            // person at the machine takes their desktop back from the agent.
            // The reverse is refused, whoever calls.
            let user_takes_from_the_agent =
                holder == ControlHolder::User && driver.holder == ControlHolder::Agent;
            if driver.holder != holder && !user_takes_from_the_agent {
                return Err(ScreenError::ControlHeld {
                    source_id: source.to_string(),
                    holder: driver.holder,
                    asked: holder,
                }
                .into());
            }
        }
        controllers.insert(source.to_string(), Driver { controller, holder });
        Ok(())
    }

    /// Release `holder`'s control of `source`, removing its controller so the
    /// source is view-only again.
    ///
    /// Releasing a source nobody drives is a no-op. A release by the holder that
    /// is *not* driving fails with [`ScreenError::ControlHeld`], so neither side
    /// takes the other's input away behind its back.
    pub fn release_control(&self, source: &str, holder: ControlHolder) -> Result<()> {
        let mut controllers = self.controllers.write().unwrap();
        if let Some(driver) = controllers.get(source) {
            if driver.holder != holder {
                return Err(ScreenError::ControlHeld {
                    source_id: source.to_string(),
                    holder: driver.holder,
                    asked: holder,
                }
                .into());
            }
        }
        controllers.remove(source);
        Ok(())
    }

    /// Remove `source`'s controller and return it, leaving the source view-only.
    ///
    /// The primitive [`Self::release_control`] removes through.
    pub fn unregister_controller(&self, source: &str) -> Option<Arc<dyn ScreenController>> {
        self.controllers
            .write()
            .unwrap()
            .remove(source)
            .map(|driver| driver.controller)
    }

    /// Remove `source` from the registry: its capturer and its controller are
    /// dropped with the entry, so a closed desktop's frame buffer — and, for a
    /// remote source, the client that feeds it — goes with them.
    ///
    /// This is what closing a desktop is. Every read of the source afterwards
    /// fails with [`ScreenError::NoCapturer`] and every input call with
    /// [`ScreenError::NoController`], instead of serving the last frame of a
    /// desktop nobody is watching. Control goes with the entry too, whichever
    /// holder had it, because the entry is what a take registered into.
    ///
    /// Returns whether anything was registered under `source`; removing a
    /// source twice, or one that was never registered, is a no-op.
    pub fn unregister_source(&self, source: &str) -> bool {
        let capturer = self.capturers.write().unwrap().remove(source).is_some();
        let controller = self.controllers.write().unwrap().remove(source).is_some();
        capturer || controller
    }

    /// Who is driving `source`'s input, or `None` when it is view-only.
    pub fn control_holder(&self, source: &str) -> Option<ControlHolder> {
        self.controllers
            .read()
            .unwrap()
            .get(source)
            .map(|driver| driver.holder)
    }

    /// Whether anything is driving `source`'s input.
    ///
    /// A source whose capturer is registered and whose controller is not is
    /// view-only: `has_control` answers `false` and input calls fail.
    pub fn has_control(&self, source: &str) -> bool {
        self.control_holder(source).is_some()
    }

    /// Resolve a capturer by source.
    pub fn capturer(&self, source: &str) -> Option<Arc<dyn ScreenCapturer>> {
        self.capturers.read().unwrap().get(source).cloned()
    }

    /// Resolve a controller by source, whoever holds it.
    pub fn controller(&self, source: &str) -> Option<Arc<dyn ScreenController>> {
        self.controllers
            .read()
            .unwrap()
            .get(source)
            .map(|driver| Arc::clone(&driver.controller))
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

    /// Click at `(x, y)` on `source` as the user's own path — the screen pane.
    pub fn click(&self, source: &str, x: u32, y: u32) -> Result<()> {
        self.click_as(ControlHolder::User, source, x, y)
    }

    /// Type `text` into `source` as the user's own path — the screen pane.
    pub fn type_text(&self, source: &str, text: &str) -> Result<()> {
        self.type_text_as(ControlHolder::User, source, text)
    }

    /// Scroll on `source` as the user's own path — the screen pane.
    pub fn scroll(&self, source: &str, dx: i32, dy: i32) -> Result<()> {
        self.scroll_as(ControlHolder::User, source, dx, dy)
    }

    /// Click at `(x, y)` on `source` as `holder`.
    ///
    /// Fails with [`ScreenError::ControlHeld`] while the other holder is
    /// driving, so an input call made while the other side holds the desktop is
    /// an error the caller can act on, never a silent no-op. The agent's path
    /// drives through this with [`ControlHolder::Agent`].
    pub fn click_as(&self, holder: ControlHolder, source: &str, x: u32, y: u32) -> Result<()> {
        self.driver(holder, source)?.click(x, y)
    }

    /// Type `text` into `source` as `holder`. See [`Self::click_as`].
    pub fn type_text_as(&self, holder: ControlHolder, source: &str, text: &str) -> Result<()> {
        self.driver(holder, source)?.type_text(text)
    }

    /// Scroll on `source` as `holder`. See [`Self::click_as`].
    pub fn scroll_as(&self, holder: ControlHolder, source: &str, dx: i32, dy: i32) -> Result<()> {
        self.driver(holder, source)?.scroll(dx, dy)
    }

    /// Resolve `source`'s controller for `holder`: absent means view-only, held
    /// by the other side means refused.
    fn driver(&self, holder: ControlHolder, source: &str) -> Result<Arc<dyn ScreenController>> {
        let controllers = self.controllers.read().unwrap();
        match controllers.get(source) {
            Some(driver) if driver.holder != holder => Err(ScreenError::ControlHeld {
                source_id: source.to_string(),
                holder: driver.holder,
                asked: holder,
            }
            .into()),
            Some(driver) => Ok(Arc::clone(&driver.controller)),
            None => Err(ScreenError::NoController(source.to_string()).into()),
        }
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

    /// Every input method of a source that has a capturer and no controller: a
    /// fresh remote desktop is view-only, not silently drivable.
    fn assert_view_only(reg: &ScreenRegistry, source: &str) {
        let errors = [
            reg.click(source, 1, 2).unwrap_err(),
            reg.type_text(source, "x").unwrap_err(),
            reg.scroll(source, 0, -1).unwrap_err(),
        ];
        for err in errors {
            assert!(
                matches!(
                    err.downcast_ref::<ScreenError>(),
                    Some(ScreenError::NoController(s)) if s == source
                ),
                "{err}"
            );
        }
    }

    #[test]
    fn fresh_source_is_view_only_until_control_is_taken() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(
            MockCapturer::new("rdp:vm").with_frame(frame("rdp:vm", 1, 1, 9)),
        ));

        // A capturer and no controller: reading works, driving does not.
        assert_eq!(reg.capture("rdp:vm").unwrap().data[0], 9);
        assert!(!reg.has_control("rdp:vm"));
        assert_eq!(reg.control_holder("rdp:vm"), None);
        assert_view_only(&reg, "rdp:vm");

        // Taking the desktop hands input to exactly one holder.
        let user = Arc::new(MockController::new("rdp:vm"));
        reg.take_control("rdp:vm", ControlHolder::User, user.clone())
            .unwrap();
        assert!(reg.has_control("rdp:vm"));
        assert_eq!(reg.control_holder("rdp:vm"), Some(ControlHolder::User));
        reg.click("rdp:vm", 3, 4).unwrap();
        reg.type_text("rdp:vm", "hi").unwrap();
        reg.scroll("rdp:vm", 1, -1).unwrap();
        assert_eq!(user.actions().len(), 3);

        // Releasing takes the controller out and leaves the frame in place.
        reg.release_control("rdp:vm", ControlHolder::User).unwrap();
        assert!(reg.controller_sources().is_empty());
        assert!(!reg.has_control("rdp:vm"));
        assert_view_only(&reg, "rdp:vm");
        assert_eq!(reg.capture("rdp:vm").unwrap().data[0], 9);

        // Releasing a source nobody drives is a no-op.
        reg.release_control("rdp:vm", ControlHolder::User).unwrap();
    }

    #[test]
    fn two_takes_do_not_both_hold() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(MockCapturer::new("rdp:vm")));

        let user = Arc::new(MockController::new("rdp:vm"));
        let agent = Arc::new(MockController::new("rdp:vm"));
        reg.take_control("rdp:vm", ControlHolder::User, user.clone())
            .unwrap();

        // The agent's ask is refused before it registers anything: the user
        // keeps the desktop and the agent's controller never drives it.
        let err = reg
            .take_control("rdp:vm", ControlHolder::Agent, agent.clone())
            .unwrap_err();
        assert!(
            matches!(
                err.downcast_ref::<ScreenError>(),
                Some(ScreenError::ControlHeld {
                    holder: ControlHolder::User,
                    asked: ControlHolder::Agent,
                    ..
                })
            ),
            "{err}"
        );
        assert!(err.to_string().contains("driven by the user"), "{err}");
        assert_eq!(reg.control_holder("rdp:vm"), Some(ControlHolder::User));
        assert_eq!(reg.controller_sources(), vec!["rdp:vm".to_string()]);

        // The agent's input while the user drives is a clear error, not a
        // silent no-op, and it does not reach the agent's controller.
        let err = reg
            .click_as(ControlHolder::Agent, "rdp:vm", 1, 1)
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ScreenError>(),
            Some(ScreenError::ControlHeld { .. })
        ));
        assert!(agent.actions().is_empty());

        // Nor can the agent release the user's control behind it.
        let err = reg
            .release_control("rdp:vm", ControlHolder::Agent)
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ScreenError>(),
            Some(ScreenError::ControlHeld { .. })
        ));
        assert!(reg.has_control("rdp:vm"));

        // Once the user releases, the refused ask succeeds: the desktop is the
        // agent's, and the user's own path is the one refused now.
        reg.release_control("rdp:vm", ControlHolder::User).unwrap();
        reg.take_control("rdp:vm", ControlHolder::Agent, agent.clone())
            .unwrap();
        assert_eq!(reg.control_holder("rdp:vm"), Some(ControlHolder::Agent));
        reg.click_as(ControlHolder::Agent, "rdp:vm", 5, 6).unwrap();
        assert_eq!(agent.actions(), vec![MockAction::Click { x: 5, y: 6 }]);
        let err = reg.click("rdp:vm", 5, 6).unwrap_err();
        assert!(err.to_string().contains("driven by the agent"), "{err}");

        // The holder already driving may take again, which replaces its
        // controller (a reconnect) rather than being refused.
        let again = Arc::new(MockController::new("rdp:vm"));
        reg.take_control("rdp:vm", ControlHolder::Agent, again.clone())
            .unwrap();
        reg.click_as(ControlHolder::Agent, "rdp:vm", 7, 8).unwrap();
        assert_eq!(again.actions(), vec![MockAction::Click { x: 7, y: 8 }]);
        assert_eq!(reg.controller_sources(), vec!["rdp:vm".to_string()]);
    }

    /// The recorded takeover rule, at the authority that enforces it: the user
    /// may take the desktop back from the agent at any time, and the agent may
    /// never take it from the user.
    #[test]
    fn the_user_takes_the_desktop_back_from_the_agent_at_any_time() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(MockCapturer::new("rdp:vm")));
        let agent = Arc::new(MockController::new("rdp:vm"));
        let user = Arc::new(MockController::new("rdp:vm"));

        reg.take_control("rdp:vm", ControlHolder::Agent, agent.clone())
            .unwrap();
        reg.click_as(ControlHolder::Agent, "rdp:vm", 1, 1).unwrap();
        assert_eq!(reg.control_holder("rdp:vm"), Some(ControlHolder::Agent));

        // The user's take is not refused: it replaces the agent's hold, and the
        // user's input reaches the controller they brought.
        reg.take_control("rdp:vm", ControlHolder::User, user.clone())
            .unwrap();
        assert_eq!(reg.control_holder("rdp:vm"), Some(ControlHolder::User));
        reg.click("rdp:vm", 2, 2).unwrap();
        assert_eq!(user.actions(), vec![MockAction::Click { x: 2, y: 2 }]);

        // The agent is the side refused now — its take and its input alike, and
        // neither registers nor drives anything.
        let agent_again = Arc::new(MockController::new("rdp:vm"));
        let err = reg
            .take_control("rdp:vm", ControlHolder::Agent, agent_again.clone())
            .unwrap_err();
        assert!(
            matches!(
                err.downcast_ref::<ScreenError>(),
                Some(ScreenError::ControlHeld {
                    holder: ControlHolder::User,
                    asked: ControlHolder::Agent,
                    ..
                })
            ),
            "{err}"
        );
        assert!(
            err.to_string().contains("driven by the user"),
            "the refused agent take says who holds it: {err}"
        );
        let err = reg
            .click_as(ControlHolder::Agent, "rdp:vm", 3, 3)
            .unwrap_err();
        assert!(err.to_string().contains("driven by the user"), "{err}");
        assert!(agent_again.actions().is_empty());
        assert_eq!(
            agent.actions(),
            vec![MockAction::Click { x: 1, y: 1 }],
            "the agent keeps only what it drove while it held the desktop"
        );
        assert_eq!(reg.controller_sources(), vec!["rdp:vm".to_string()]);
    }

    #[test]
    fn unregister_controller_removes_the_writer_and_keeps_the_frame() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(MockCapturer::new("display:0").with_blank(2, 2)));
        let ctl: Arc<dyn ScreenController> = Arc::new(MockController::new("display:0"));
        reg.register_controller(Arc::clone(&ctl));
        assert_eq!(reg.control_holder("display:0"), Some(ControlHolder::User));

        assert!(reg.unregister_controller("display:0").is_some());
        assert!(reg.unregister_controller("display:0").is_none());
        assert!(!reg.has_control("display:0"));
        assert_eq!(reg.capture("display:0").unwrap().len(), 16);
        assert_view_only(&reg, "display:0");
    }

    /// A closed source is gone from both maps: its capturer (and with it the
    /// frame buffer) and its controller are dropped, so nothing keeps serving
    /// the last frame of a desktop nobody watches.
    #[test]
    fn unregister_source_removes_the_capturer_and_the_controller() {
        let reg = ScreenRegistry::new();
        let source = "remote-xrdp:vm:3389";
        reg.register_capturer(Arc::new(
            MockCapturer::new(source).with_frame(frame(source, 2, 2, 9)),
        ));
        let ctl = Arc::new(MockController::new(source));
        reg.register_controller(ctl.clone());
        assert_eq!(reg.capture(source).unwrap().data[0], 9);

        assert!(reg.unregister_source(source));
        assert!(reg.capturer(source).is_none());
        assert!(reg.controller(source).is_none());
        assert!(reg.capturer_sources().is_empty());
        assert!(reg.controller_sources().is_empty());
        // Both halves are unreachable now: no stale frame, no writer.
        let err = reg.capture(source).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ScreenError>(),
            Some(ScreenError::NoCapturer(s)) if s == source
        ));
        assert_view_only(&reg, source);
        // Removing it again, or one that never existed, is a no-op.
        assert!(!reg.unregister_source(source));
        assert!(!reg.unregister_source("never-registered"));
    }

    /// The close is "remove the source", not "release the control": a view-only
    /// source (the fresh remote desktop) and a controller-only one both go.
    #[test]
    fn unregister_source_removes_a_view_only_source_and_a_controller_only_one() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(
            MockCapturer::new("rdp:view-only").with_blank(1, 1),
        ));
        assert!(reg.unregister_source("rdp:view-only"));
        assert!(matches!(
            reg.capture("rdp:view-only")
                .unwrap_err()
                .downcast_ref::<ScreenError>(),
            Some(ScreenError::NoCapturer(s)) if s == "rdp:view-only"
        ));

        reg.register_controller(Arc::new(MockController::new("rdp:writer-only")));
        assert!(reg.unregister_source("rdp:writer-only"));
        assert!(reg.controller_sources().is_empty());
        assert!(!reg.has_control("rdp:writer-only"));
    }

    /// The raw `register_controller` is the own-desktop path the local adapter
    /// takes: the person at the machine is the only writer there.
    #[test]
    fn a_registered_controller_is_the_own_desktops_user() {
        let reg = ScreenRegistry::new();
        reg.register_capturer(Arc::new(MockCapturer::new("local")));
        let local = Arc::new(MockController::new("local"));
        reg.register_controller(local.clone());

        assert_eq!(reg.control_holder("local"), Some(ControlHolder::User));
        assert!(reg.has_control("local"));
        reg.click("local", 1, 2).unwrap();
        assert_eq!(local.actions(), vec![MockAction::Click { x: 1, y: 2 }]);
    }

    #[test]
    fn a_refused_holder_reads_in_one_sentence() {
        let err = ScreenError::ControlHeld {
            source_id: "rdp:vm".to_string(),
            holder: ControlHolder::User,
            asked: ControlHolder::Agent,
        };
        assert_eq!(
            err.to_string(),
            "source 'rdp:vm' is driven by the user: the agent cannot drive it until that control \
             is released"
        );
    }

    /// A client's liveness is one shared fact: the path that owns the session
    /// records the client going and every holder of the handle sees it, which
    /// is what lets a desktop whose client is gone be reported rather than
    /// drawn.
    #[test]
    fn client_liveness_is_shared_and_records_the_client_going() {
        let liveness = ClientLiveness::new();
        let held = liveness.clone();
        assert!(liveness.is_alive(), "a fresh client is there");
        assert!(ClientLiveness::default().is_alive());

        liveness.mark_gone();
        assert!(!held.is_alive(), "the holder sees the client go");
        assert_eq!(
            ScreenError::ClientGone("remote-xrdp:vm:3389".to_string()).to_string(),
            "the client behind source 'remote-xrdp:vm:3389' is gone: the desktop ended, so no \
             frame is served after it"
        );
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
