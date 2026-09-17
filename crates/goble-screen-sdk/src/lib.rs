//! Remote screen source SDK: a real remote desktop (xrdp / RDP) exposed as a
//! [`ScreenCapturer`] + [`ScreenController`] pair, backed by the IronRDP client
//! engine.
//!
//! The crate sits in the `runtime` layer of the `types <- protocol <- runtime`
//! split for screen I/O, on the *remote* side: it turns a decoded RDP desktop
//! into the same [`ScreenFrame`] shape the local adapter produces, so a
//! [`ScreenRegistry`] can host a remote source next to the local one and the
//! app treats them identically.
//!
//! A remote desktop has **two** sides that can reach it — the agent that asked
//! for it and the user watching it — so connecting registers the **capturer
//! only**: a freshly opened desktop is view-only, and the caller keeps the
//! controller and hands it to [`ScreenRegistry::take_control`] when someone
//! takes the desktop (one writer at a time, see `goble-screen-core`). The local
//! adapter's own desktop is the opposite case and registers both halves,
//! because there is only one person at the machine to drive it.
//!
//! Closing one is dropping its controller: [`RdpController`]'s `Drop` sends the
//! engine's in-band stop, so the client ends the connection (and the host's
//! session with it) instead of leaving it running behind a card nobody watches.
//! The registry's copy goes with [`ScreenRegistry::unregister_source`].
//!
//! A client can also just go. The `ClientLiveness` handle
//! [`RdpRemoteSource::connect`] hands back is marked gone when the session
//! fails or ends, and the capturer then reports that
//! ([`goble_screen_core::ScreenError::ClientGone`]) instead of serving the
//! frame the dead session left behind.
//!
//! The actual RDP client is gated behind the `rdp` feature (off by default)
//! because it is a heavy dependency tree. Without `rdp`, the crate still builds
//! with a [`StubRemoteSource`] that reports an error, so a workspace-wide
//! `cargo check` stays light and this crate always compiles.

use std::sync::Arc;

#[cfg(feature = "rdp")]
use anyhow::Context;
use anyhow::Result;
#[cfg(feature = "rdp")]
use goble_screen_core::{ClientLiveness, ScreenError};
use goble_screen_core::{ScreenCapturer, ScreenController, ScreenFrame, ScreenRegistry};

/// How to reach a remote RDP desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfig {
    /// Hostname or IP of the remote (e.g. `"vm.example.com"`).
    pub host: String,
    /// RDP port (default 3389).
    pub port: u16,
    /// RDP account username.
    pub username: String,
    /// RDP account password.
    pub password: String,
    /// Requested desktop width in pixels.
    pub width: u16,
    /// Requested desktop height in pixels.
    pub height: u16,
}

impl RemoteConfig {
    /// A config for the default RDP port and a 1280x720 desktop.
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port: 3389,
            username: username.into(),
            password: password.into(),
            width: 1280,
            height: 720,
        }
    }
}

// ---------------------------------------------------------------------------
// Real IronRDP-backed source (`rdp` feature)
// ---------------------------------------------------------------------------

/// A live remote screen pair: [`RdpRemoteSource::capturer`] yields the most
/// recent decoded desktop as an RGBA8 [`ScreenFrame`]; [`RdpRemoteSource::controller`]
/// forwards click/type/scroll to the remote session.
///
/// Built by [`RdpRemoteSource::connect`], which spawns the IronRDP client on a
/// background thread and registers the **capturer only** into a
/// [`ScreenRegistry`]. The returned controller is the caller's: a freshly
/// opened desktop is view-only until a holder takes it through
/// [`ScreenRegistry::take_control`], which is what keeps the agent and the user
/// from driving it at the same time.
#[cfg(feature = "rdp")]
pub struct RdpRemoteSource {
    /// Captures the most recent decoded frame. It refuses to serve a frame once
    /// the client is gone ([`goble_screen_core::ClientLiveness`]), so a dead
    /// session's last desktop is reported, not drawn.
    pub capturer: Arc<dyn ScreenCapturer>,
    /// Drives remote pointer/keyboard input. Registered only while a holder has
    /// taken the desktop.
    pub controller: Arc<dyn ScreenController>,
    /// Whether the client behind this source is still there. The caller that
    /// keeps the desktop records this handle: it is marked gone when the
    /// session fails or ends, and it is what answers "this desktop's client is
    /// gone" without polling the client.
    pub liveness: ClientLiveness,
}

#[cfg(feature = "rdp")]
impl RdpRemoteSource {
    /// Build and spawn the client for `config`, registering the capturer under
    /// `source` into `registry`. Returns the source so the caller holds the
    /// controller it hands to [`ScreenRegistry::take_control`].
    pub fn connect(
        source: &str,
        config: RemoteConfig,
        registry: &ScreenRegistry,
    ) -> Result<Self> {
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(8);
        let rdpc = build_ironrdp_client(&config, output_tx)?;
        // The client exposes its input channel up-front; the controller shares it
        // so mouse/keyboard reach the exact task that owns the session.
        let input_sender = rdpc.input_sender();

        let liveness = ClientLiveness::new();
        let capturer: Arc<dyn ScreenCapturer> =
            RdpCapturer::start(source, config, rdpc, output_rx, liveness.clone())?;
        let controller: Arc<dyn ScreenController> =
            Arc::new(RdpController::new(source, input_sender));

        // The capturer alone. Registering the controller here too is what made a
        // handoff land with the desktop drivable by the agent and the user at
        // once; a source with a capturer and no controller is view-only, and
        // control is registered on a take.
        registry.register_capturer(Arc::clone(&capturer));

        Ok(Self {
            capturer,
            controller,
            liveness,
        })
    }
}

/// Capture-side handle for a remote desktop.
#[cfg(feature = "rdp")]
struct RdpCapturer {
    source: String,
    latest: Arc<std::sync::Mutex<ScreenFrame>>,
    /// Whether the client is still there. Marked gone when the session fails,
    /// ends, or its engine stops feeding frames, and consulted before a frame
    /// is served.
    liveness: ClientLiveness,
    /// Kept so the drain task's runtime stays alive for the source's life.
    _runtime: Arc<tokio::runtime::Runtime>,
    /// The OS thread running the IronRDP client engine (detached on drop).
    _client_thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(feature = "rdp")]
impl RdpCapturer {
    /// Spawn the IronRDP client and return the capture handle.
    ///
    /// Mirrors `ironrdp-viewer` / `ironrdp-agent`: the connection future runs on
    /// a current-thread runtime on a dedicated OS thread via `block_on`, which
    /// sidesteps any `Send` requirement the connection future has. A second task
    /// (on an outer runtime) drains the decoded-frame channel and keeps `latest`
    /// fresh, so `capture()` always reflects the live desktop.
    fn start(
        source: &str,
        config: RemoteConfig,
        rdpc: ironrdp_client::rdp::RdpClient,
        mut output_rx: tokio::sync::mpsc::Receiver<ironrdp_client::rdp::RdpOutputEvent>,
        liveness: ClientLiveness,
    ) -> Result<Arc<Self>> {
        let latest: Arc<std::sync::Mutex<ScreenFrame>> =
            Arc::new(std::sync::Mutex::new(ScreenFrame::blank(source, config.width as u32, config.height as u32)));

        // Client engine thread.
        let session_name = format!("goble-rdp-{source}");
        let client_thread = std::thread::Builder::new()
            .name(session_name)
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt.block_on(rdpc.run()),
                    Err(e) => log::error!("failed to build RDP session runtime: {e}"),
                }
            })
            .context("failed to spawn RDP client thread")?;

        // Decoded-frame drain on an outer runtime.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to build RDP output runtime")?;
        let latest_clone = Arc::clone(&latest);
        let source_owned = source.to_string();
        let (blank_w, blank_h) = (config.width as u32, config.height as u32);
        let gone = liveness.clone();
        runtime.spawn(async move {
            while let Some(event) = output_rx.recv().await {
                match event {
                    ironrdp_client::rdp::RdpOutputEvent::Image {
                        buffer,
                        width,
                        height,
                    } => {
                        let frame = rgba32_to_frame(
                            &source_owned,
                            width.get() as u32,
                            height.get() as u32,
                            &buffer,
                        );
                        if let Ok(mut f) = latest_clone.lock() {
                            *f = frame;
                        }
                    }
                    ironrdp_client::rdp::RdpOutputEvent::ConnectionFailure(e) => {
                        log::warn!("rdp connection failed: {e}");
                        if let Ok(mut f) = latest_clone.lock() {
                            *f = ScreenFrame::blank(source_owned.clone(), blank_w, blank_h);
                        }
                        break;
                    }
                    ironrdp_client::rdp::RdpOutputEvent::Terminated(reason) => {
                        log::debug!("rdp terminated: {reason:?}");
                        break;
                    }
                    _ => {}
                }
            }
            // The engine holds the channel's sender for as long as it runs, so
            // the loop ending — a failed or ended session, or a client thread
            // that never started — is the client gone. From here the capturer
            // refuses to hand out the frame it left behind.
            gone.mark_gone();
        });

        Ok(Arc::new(Self {
            source: source.to_string(),
            latest,
            liveness,
            _runtime: Arc::new(runtime),
            _client_thread: Some(client_thread),
        }))
    }
}

#[cfg(feature = "rdp")]
impl ScreenCapturer for RdpCapturer {
    fn source(&self) -> &str {
        &self.source
    }

    fn capture(&self) -> Result<ScreenFrame> {
        if !self.liveness.is_alive() {
            return Err(ScreenError::ClientGone(self.source.clone()).into());
        }
        Ok(self
            .latest
            .lock()
            .map(|f| f.clone())
            .unwrap_or_else(|_| ScreenFrame::blank(self.source.clone(), 0, 0)))
    }
}

/// Input-side handle for a remote desktop.
#[cfg(feature = "rdp")]
struct RdpController {
    source: String,
    input_sender: tokio::sync::mpsc::UnboundedSender<ironrdp_client::rdp::RdpInputEvent>,
    input_db: std::sync::Mutex<ironrdp_input::Database>,
}

#[cfg(feature = "rdp")]
impl RdpController {
    fn new(
        source: &str,
        input_sender: tokio::sync::mpsc::UnboundedSender<ironrdp_client::rdp::RdpInputEvent>,
    ) -> Self {
        Self {
            source: source.to_string(),
            input_sender,
            input_db: std::sync::Mutex::new(ironrdp_input::Database::new()),
        }
    }

    fn push(
        &self,
        events: smallvec::SmallVec<[ironrdp_pdu::input::fast_path::FastPathInputEvent; 2]>,
    ) {
        use ironrdp_client::rdp::RdpInputEvent;
        if events.is_empty() {
            return;
        }
        let _ = self.input_sender.send(RdpInputEvent::FastPath(events));
    }
}

#[cfg(feature = "rdp")]
impl ScreenController for RdpController {
    fn source(&self) -> &str {
        &self.source
    }

    fn click(&self, x: u32, y: u32) -> Result<()> {
        use ironrdp_input::{MouseButton, MousePosition, Operation};
        let position = MousePosition {
            x: x as u16,
            y: y as u16,
        };
        let events = self
            .input_db
            .lock()
            .map_err(|e| anyhow::anyhow!("input db poisoned: {e}"))?
            .apply([
                Operation::MouseMove(position),
                Operation::MouseButtonPressed(MouseButton::Left),
                Operation::MouseButtonReleased(MouseButton::Left),
            ]);
        self.push(events);
        Ok(())
    }

    fn type_text(&self, text: &str) -> Result<()> {
        use ironrdp_input::Operation;
        let mut db = self
            .input_db
            .lock()
            .map_err(|e| anyhow::anyhow!("input db poisoned: {e}"))?;
        let mut events = smallvec::SmallVec::new();
        for c in text.chars() {
            for ev in db.apply([Operation::UnicodeKeyPressed(c), Operation::UnicodeKeyReleased(c)]) {
                events.push(ev);
            }
        }
        drop(db);
        self.push(events);
        Ok(())
    }

    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        use ironrdp_input::{Operation, WheelRotations};
        let mut events = smallvec::SmallVec::new();
        let mut db = self
            .input_db
            .lock()
            .map_err(|e| anyhow::anyhow!("input db poisoned: {e}"))?;
        if dy != 0 {
            events.extend(db.apply([Operation::WheelRotations(WheelRotations {
                is_vertical: true,
                rotation_units: dy as i16,
            })]));
        }
        if dx != 0 {
            events.extend(db.apply([Operation::WheelRotations(WheelRotations {
                is_vertical: false,
                rotation_units: dx as i16,
            })]));
        }
        drop(db);
        self.push(events);
        Ok(())
    }
}

#[cfg(feature = "rdp")]
impl Drop for RdpController {
    /// The close reaches the client engine, not just our registry: the engine's
    /// in-band stop is [`RdpInputEvent::Close`], which makes its session loop
    /// send the RDP shutdown request and end the connection — the socket closing
    /// is what ends the host's `xrdp` session instead of leaving it running
    /// behind a card nobody is watching ([`ironrdp_client::rdp::RdpClient::run`]).
    ///
    /// The controller is a source's write end, so its drop is the close.
    /// `ScreenRegistry::unregister_source` drops the registry's copy and
    /// `DesktopState::close_remote_screen` the one parked for the source; a
    /// `release_control` between the two drops neither, because the other copy
    /// is still alive.
    fn drop(&mut self) {
        let _ = self
            .input_sender
            .send(ironrdp_client::rdp::RdpInputEvent::Close);
    }
}

// ---------------------------------------------------------------------------
// Stub source when the `rdp` feature is disabled
// ---------------------------------------------------------------------------

/// A source that cannot connect until the `rdp` feature is enabled. Without the
/// feature this is what the crate exposes, so it always builds and the app can
/// install the same registry wiring; connecting fails at runtime.
///
/// Its shape is the real source's: the registry gets the capturer alone (a
/// desktop that never connected has no writer either), and the controller is
/// the caller's handle, taken and released like any other.
#[cfg(not(feature = "rdp"))]
pub struct StubRemoteSource {
    /// Captures the placeholder frame; the registry takes this one.
    pub capturer: Arc<dyn ScreenCapturer>,
    /// The input handle the caller would take the desktop with. Nothing is
    /// connected, so it drives nothing.
    pub controller: Arc<dyn ScreenController>,
}

#[cfg(not(feature = "rdp"))]
impl StubRemoteSource {
    /// Create a stub whose capturer warns that it `requires the rdp feature`.
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let error = "remote screen requires the `rdp` cargo feature".to_string();
        Self {
            capturer: Arc::new(StubCapturer::new(&source, error)),
            controller: Arc::new(StubController::new(&source)),
        }
    }

    /// Register the stub's capturer into `registry` (the controller stays the
    /// caller's), returning the source. Connecting always fails, which surfaces
    /// as a blank frame, and the registered source is view-only.
    pub fn connect_and_register(
        source: &str,
        _config: RemoteConfig,
        registry: &ScreenRegistry,
    ) -> Result<Self> {
        let this = Self::new(source);
        registry.register_capturer(Arc::clone(&this.capturer));
        Ok(this)
    }
}

#[cfg(not(feature = "rdp"))]
struct StubCapturer {
    source: String,
    message: String,
}

#[cfg(not(feature = "rdp"))]
impl StubCapturer {
    fn new(source: &str, message: String) -> Self {
        Self {
            source: source.to_string(),
            message,
        }
    }
}

#[cfg(not(feature = "rdp"))]
impl ScreenCapturer for StubCapturer {
    fn source(&self) -> &str {
        &self.source
    }

    fn capture(&self) -> Result<ScreenFrame> {
        log::warn!("{}", self.message);
        Ok(ScreenFrame::blank(self.source.clone(), 0, 0))
    }
}

#[cfg(not(feature = "rdp"))]
struct StubController {
    source: String,
}

#[cfg(not(feature = "rdp"))]
impl StubController {
    fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
        }
    }
}

#[cfg(not(feature = "rdp"))]
impl ScreenController for StubController {
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

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build the IronRDP client engine for `config` with `output_tx` as its output
/// channel. The returned [`RdpClient::input_sender`] (via [`RdpClient::input_sender`])
/// drives remote input.
#[cfg(feature = "rdp")]
fn build_ironrdp_client(
    config: &RemoteConfig,
    output_tx: tokio::sync::mpsc::Sender<ironrdp_client::rdp::RdpOutputEvent>,
) -> Result<ironrdp_client::rdp::RdpClient> {
    use ironrdp_client::config::{ConfigBuilder, Destination};
    use ironrdp_pdu::rdp::capability_sets::MajorPlatformType;

    let destination = Destination::from_parts(config.host.clone(), config.port);
    let cfg = ConfigBuilder::new()
        .with_destination(destination)
        .with_username(config.username.clone())
        .with_password(config.password.clone())
        .with_client_name("goble")
        .with_client_dir("~/.goble")
        .with_client_build(2600)
        .with_platform(MajorPlatformType::OSX)
        .with_desktop_width(config.width)
        .with_desktop_height(config.height)
        .with_credssp(true)
        .with_tls(true)
        // A viewer that was handed an account logs in with it. Without the
        // Client Info PDU's `AUTOLOGON` flag xrdp does not start a session for
        // `config.username` at all: it draws its own login window instead, whose
        // bitmap update the engine cannot decode, so no frame ever arrives.
        .with_autologon(true)
        .build()
        .context("failed to build IronRDP config")?;
    Ok(ironrdp_client::rdp::RdpClient::new(cfg, output_tx))
}

/// Convert a `Vec<u32>` packed frame (IronRDP's `RgbA32` output format) into the
/// RGBA8 [`ScreenFrame`] the app renders. Each `u32` is `0x00RRGGBB` (the `A`
/// byte is discarded by the source, so alpha is set to opaque).
#[cfg(feature = "rdp")]
fn rgba32_to_frame(source: &str, width: u32, height: u32, buffer: &[u32]) -> ScreenFrame {
    let pixel_count = (width as usize) * (height as usize);
    let mut data = vec![0u8; pixel_count * 4];
    for (i, &px) in buffer.iter().take(pixel_count).enumerate() {
        let index = i * 4;
        // px = 0x00 RR GG BB
        data[index] = ((px >> 16) & 0xff) as u8;
        data[index + 1] = ((px >> 8) & 0xff) as u8;
        data[index + 2] = (px & 0xff) as u8;
        data[index + 3] = 0xff;
    }
    ScreenFrame::new(source, width, height, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_screen_core::{ControlHolder, ScreenError};

    #[test]
    fn remote_config_defaults() {
        let cfg = RemoteConfig::new("vm.example.com", "user", "pass");
        assert_eq!(cfg.port, 3389);
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);
    }

    #[cfg(feature = "rdp")]
    #[test]
    fn rgba32_to_frame_unpacks_rgba8() {
        // 0x00 FF 00 00 => red 255, green 0, blue 0, alpha 255.
        let frame = rgba32_to_frame("d", 1, 1, &[0x00ff0000]);
        assert_eq!(frame.width, 1);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.data, vec![255, 0, 0, 255]);
    }

    #[cfg(feature = "rdp")]
    #[test]
    fn rgba32_to_frame_unpacks_two_pixels() {
        let frame = rgba32_to_frame("d", 2, 1, &[0x00ff0000, 0x0000ff00]);
        assert_eq!(frame.data, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    }

    /// A remote source registers its **capturer alone**: the desktop is
    /// view-only until a holder takes it, and the refused take is how the other
    /// side asks for it. In the default build the stub stands in for the real
    /// client; the `rdp` build has the same assertion over `connect`.
    #[cfg(not(feature = "rdp"))]
    #[test]
    fn stub_source_is_view_only_until_control_is_taken() -> Result<()> {
        let reg = ScreenRegistry::new();
        let source =
            StubRemoteSource::connect_and_register("vm", RemoteConfig::new("h", "u", "p"), &reg)?;

        assert_eq!(reg.capturer_sources(), vec!["vm".to_string()]);
        assert!(
            reg.controller_sources().is_empty(),
            "connecting must register no writer"
        );
        assert!(!reg.has_control("vm"));
        assert_eq!(reg.control_holder("vm"), None);
        // Reading works — the frame is the blank placeholder — and input does not.
        assert!(reg.capture("vm").unwrap().is_empty());
        assert!(matches!(
            reg.click("vm", 1, 2)
                .unwrap_err()
                .downcast_ref::<ScreenError>(),
            Some(ScreenError::NoController(s)) if s == "vm"
        ));

        // Taking control registers the controller the caller holds; releasing
        // removes it and the source is view-only again.
        reg.take_control("vm", ControlHolder::User, Arc::clone(&source.controller))?;
        assert!(reg.has_control("vm"));
        assert_eq!(reg.control_holder("vm"), Some(ControlHolder::User));
        reg.click("vm", 1, 2)?;
        reg.release_control("vm", ControlHolder::User)?;
        assert!(!reg.has_control("vm"));
        assert!(reg.click("vm", 1, 2).is_err());
        Ok(())
    }

    #[cfg(not(feature = "rdp"))]
    #[test]
    fn a_second_take_does_not_hold() -> Result<()> {
        let reg = ScreenRegistry::new();
        let source =
            StubRemoteSource::connect_and_register("vm", RemoteConfig::new("h", "u", "p"), &reg)?;
        let agent: Arc<dyn ScreenController> =
            Arc::new(goble_screen_core::MockController::new("vm"));

        reg.take_control("vm", ControlHolder::User, Arc::clone(&source.controller))?;
        let err = reg
            .take_control("vm", ControlHolder::Agent, agent)
            .unwrap_err();
        assert!(
            matches!(
                err.downcast_ref::<ScreenError>(),
                Some(ScreenError::ControlHeld {
                    holder: ControlHolder::User,
                    ..
                })
            ),
            "{err}"
        );
        assert_eq!(reg.control_holder("vm"), Some(ControlHolder::User));
        // The agent's input while the user drives is an error it can act on,
        // not a silent no-op.
        assert!(matches!(
            reg.click_as(ControlHolder::Agent, "vm", 1, 2)
                .unwrap_err()
                .downcast_ref::<ScreenError>(),
            Some(ScreenError::ControlHeld { .. })
        ));
        Ok(())
    }

    #[cfg(feature = "rdp")]
    #[test]
    fn connect_registers_the_capturer_alone() {
        let reg = ScreenRegistry::new();
        let mut config = RemoteConfig::new("127.0.0.1", "goble", "x");
        // Nothing listens here — the client connects lazily on its own thread,
        // so connecting still returns a source.
        config.port = 1;
        let source = RdpRemoteSource::connect("rdp:x", config, &reg).expect("the client spawns");

        assert_eq!(reg.capturer_sources(), vec!["rdp:x".to_string()]);
        assert!(
            reg.controller_sources().is_empty(),
            "a fresh remote desktop must have no writer"
        );
        assert!(!reg.has_control("rdp:x"));
        assert!(matches!(
            reg.click("rdp:x", 1, 2)
                .unwrap_err()
                .downcast_ref::<ScreenError>(),
            Some(ScreenError::NoController(s)) if s == "rdp:x"
        ));

        // The controller comes back to the caller, and taking it registers it.
        reg.take_control(
            "rdp:x",
            ControlHolder::Agent,
            Arc::clone(&source.controller),
        )
        .unwrap();
        assert_eq!(reg.control_holder("rdp:x"), Some(ControlHolder::Agent));
        reg.release_control("rdp:x", ControlHolder::Agent).unwrap();
        assert!(!reg.has_control("rdp:x"));
    }

    /// Closing a remote desktop is not just a dropped handle: the controller's
    /// drop asks the client engine to close the session
    /// (`RdpInputEvent::Close`, the engine's in-band stop), which is what makes
    /// the connection — and with it the host's session — end. Nothing is sent
    /// while the source is open, so the close is the drop and not a heartbeat.
    #[cfg(feature = "rdp")]
    #[test]
    fn dropping_the_controller_asks_the_client_to_close() {
        use ironrdp_client::rdp::RdpInputEvent;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RdpInputEvent>();
        let controller = RdpController::new("rdp:x", tx);
        assert!(
            rx.try_recv().is_err(),
            "an open desktop sends the engine nothing"
        );

        drop(controller);
        assert!(
            matches!(rx.try_recv(), Ok(RdpInputEvent::Close)),
            "the close is the engine's own stop: {:?}",
            rx.try_recv()
        );
    }

    /// A client that is gone leaves no desktop to draw: the capturer reports it
    /// instead of serving the frame the failed session left behind. The client
    /// is real — it connects to a port nothing listens on — so the handle the
    /// caller records is the one the engine's own failure sets.
    #[cfg(feature = "rdp")]
    #[test]
    fn a_capturer_whose_client_is_gone_serves_no_frame() {
        use goble_screen_core::ClientLiveness;

        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let mut config = RemoteConfig::new("127.0.0.1", "goble", "x");
        config.port = 1;
        let rdpc = build_ironrdp_client(&config, tx).expect("the config builds");
        let liveness = ClientLiveness::new();
        let capturer = RdpCapturer::start("rdp:x", config, rdpc, rx, liveness.clone())
            .expect("the client spawns");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while liveness.is_alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "the client never went away"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let err = capturer.capture().unwrap_err();
        assert!(
            matches!(
                err.downcast_ref::<ScreenError>(),
                Some(ScreenError::ClientGone(s)) if s == "rdp:x"
            ),
            "{err}"
        );
    }
}
