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
//! The actual RDP client is gated behind the `rdp` feature (off by default)
//! because it is a heavy dependency tree. Without `rdp`, the crate still builds
//! with a [`StubRemoteSource`] that reports an error, so a workspace-wide
//! `cargo check` stays light and this crate always compiles.

use std::sync::Arc;

use anyhow::Result;
#[cfg(feature = "rdp")]
use anyhow::Context;
use goble_screen_core::{ScreenController, ScreenCapturer, ScreenFrame, ScreenRegistry};

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
/// background thread and registers both halves into a [`ScreenRegistry`].
#[cfg(feature = "rdp")]
pub struct RdpRemoteSource {
    /// Captures the most recent decoded frame.
    pub capturer: Arc<dyn ScreenCapturer>,
    /// Drives remote pointer/keyboard input.
    pub controller: Arc<dyn ScreenController>,
}

#[cfg(feature = "rdp")]
impl RdpRemoteSource {
    /// Build and spawn the client for `config`, registering the pair under
    /// `source` into `registry`. Returns the source so callers can also drive
    /// it directly.
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

        let capturer: Arc<dyn ScreenCapturer> = RdpCapturer::start(source, config, rdpc, output_rx)?;
        let controller: Arc<dyn ScreenController> = Arc::new(RdpController::new(source, input_sender));

        registry.register_capturer(Arc::clone(&capturer));
        registry.register_controller(Arc::clone(&controller));

        Ok(Self {
            capturer,
            controller,
        })
    }
}

/// Capture-side handle for a remote desktop.
#[cfg(feature = "rdp")]
struct RdpCapturer {
    source: String,
    latest: Arc<std::sync::Mutex<ScreenFrame>>,
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
        });

        Ok(Arc::new(Self {
            source: source.to_string(),
            latest,
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

// ---------------------------------------------------------------------------
// Stub source when the `rdp` feature is disabled
// ---------------------------------------------------------------------------

/// A source that cannot connect until the `rdp` feature is enabled. Without the
/// feature this is what the crate exposes, so it always builds and the app can
/// install the same registry wiring; connecting fails at runtime.
#[cfg(not(feature = "rdp"))]
pub struct StubRemoteSource {
    _source: String,
    error: String,
}

#[cfg(not(feature = "rdp"))]
impl StubRemoteSource {
    /// Create a stub that reports `requires the rdp feature` when used.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            _source: source.into(),
            error: "remote screen requires the `rdp` cargo feature".to_string(),
        }
    }

    /// Register the stub into `registry` (capturer + controller), returning the
    /// source. Connecting always fails, which surfaces as a blank frame.
    pub fn connect_and_register(
        source: &str,
        _config: RemoteConfig,
        registry: &ScreenRegistry,
    ) -> Result<Self> {
        let this = Self::new(source);
        let cap: Arc<dyn ScreenCapturer> = Arc::new(StubCapturer::new(source, this.error.clone()));
        let ctl: Arc<dyn ScreenController> = Arc::new(StubController::new(source));
        registry.register_capturer(cap);
        registry.register_controller(ctl);
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

    #[cfg(not(feature = "rdp"))]
    #[test]
    fn stub_registers_and_reports_error() -> Result<()> {
        let reg = ScreenRegistry::new();
        StubRemoteSource::connect_and_register("vm", RemoteConfig::new("h", "u", "p"), &reg)?;
        let f = reg.capture("vm").unwrap();
        assert!(f.is_empty());
        Ok(())
    }
}
