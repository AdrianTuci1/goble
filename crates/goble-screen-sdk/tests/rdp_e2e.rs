//! T4 — a real RDP connection decodes a frame from a live desktop.
//!
//! [`RdpRemoteSource::connect`] (`crates/goble-screen-sdk/src/lib.rs`, `rdp`
//! feature) had never been pointed at a desktop, so nothing knew whether the
//! IronRDP client engine, the packed-frame conversion or the `ScreenRegistry`
//! wiring work against a real server. This test connects the real source to the
//! xrdp + Xfce fixture in `deploy/xrdp-host/` — the packages and the pinned
//! `startxfce4` session that `REMOTE_DESKTOP_STEP` installs on a real host — and
//! asserts a *decoded frame*: an RGBA8 desktop buffer of the dimensions the
//! client asked the server for.
//!
//! It is `#[ignore]`d because it needs the Docker daemon. It starts the fixture
//! itself, on a free host port it picks, and stops it again; to point it at a
//! container you started by hand, set `GOBLE_RDP_E2E_PORT`:
//!
//! ```text
//! docker build -t goble-xrdp-host:t4 deploy/xrdp-host
//! docker run -d --name goble-t4-xrdp --shm-size=512m -p 127.0.0.1:13389:3389 goble-xrdp-host:t4
//! GOBLE_RDP_E2E_PORT=13389 cargo test -p goble-screen-sdk --features rdp \
//!   --test rdp_e2e -- --ignored --nocapture
//! ```
//!
//! `GOBLE_RDP_E2E_LOG` (or `RUST_LOG`) raises the logging level; at the default
//! `warn` the run still prints why the session ended, through the SDK's own
//! `log` line and the IronRDP engine's `tracing` events.
//!
//! Readiness is polled with a deadline, never slept on: on a container this test
//! started it waits for xrdp's own `listening to port` line and then for the
//! published port to answer. The container check is not redundant — Docker's
//! port forwarder accepts a host connection *before* anything inside the
//! container is listening, so a bare TCP connect to the published port succeeds
//! against a server that is not up yet.
//!
//! **The frame only decodes with `ConfigBuilder::with_autologon(true)`, which
//! T4 added.** Without the Client Info PDU's `AUTOLOGON` flag xrdp 0.9.24 never
//! starts a session for `config.username`: it draws its own login window, whose
//! ~11 KB bitmap update the engine fails to decode
//! (`BitmapData::decode` → `NotEnoughBytes { received: 1808, expected: 44430 }`)
//! and the session ends before any `Image` event. The T4 entry of
//! `.agents/10-platform-and-performance/testing-and-ci.md` records the failure
//! and the run that showed it.
//!
//! **Why the pixels are asserted and the size alone is not enough.**
//! `RdpCapturer` starts from `ScreenFrame::blank(source, config.width,
//! config.height)` and puts that same blank frame back when the connection
//! fails, so a non-empty buffer of exactly the requested dimensions exists
//! *before a single byte arrives* and again on every failed connection. A size
//! assertion would therefore pass against a server that refuses every
//! connection — the thing this item exists to tell apart. The discriminator is
//! content: the desktop paints, and the placeholder is entirely zero.
//!
//! What this does not settle: the remote host's latency, NAT/firewall behaviour,
//! and whether the pixels are *correct* — a frame that decodes is not a frame
//! that looks right.

#![cfg(feature = "rdp")]

use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use goble_screen_core::{ControlHolder, ScreenRegistry};
use goble_screen_sdk::{RdpRemoteSource, RemoteConfig};

/// The fixture image `deploy/xrdp-host/Dockerfile` builds.
const FIXTURE_IMAGE: &str = "goble-xrdp-host:t4";
/// The container the test starts when it owns the fixture.
const CONTAINER: &str = "goble-t4-xrdp";
/// The port xrdp listens on in the container (`xrdp.ini`), published to a host
/// port the test picks.
const CONTAINER_PORT: u16 = 3389;
const HOST: &str = "127.0.0.1";
/// The session account `deploy/xrdp-host/Dockerfile` creates.
const USERNAME: &str = "goble";
const PASSWORD: &str = "goble-rdp-e2e";
/// The source id the desktop is registered under, as the app registers it.
const SOURCE: &str = "rdp:xrdp-e2e";
/// The desktop the client asks the server for, and therefore the dimensions the
/// decoded frame must carry.
const WIDTH: u16 = 1024;
const HEIGHT: u16 = 768;
/// How long to wait for xrdp to accept a connection, and then for the session to
/// paint its desktop. Both are polls with a deadline, never a fixed sleep: the
/// session start (Xorg + Xfce behind `xrdp-sesman`) is what takes the time.
const READY_DEADLINE: Duration = Duration::from_secs(60);
const FRAME_DEADLINE: Duration = Duration::from_secs(180);
/// How much of the desktop must differ from the placeholder before this test
/// calls it a decoded desktop. Measured on the fixture: xrdp's own
/// connection-progress window paints 30 144 pixels in 5 colours about a second
/// after the session starts, and the settled Xfce desktop paints all 786 432 in
/// ~7 800 colours about half a second later, so half the screen separates the
/// desktop from the window with room on both sides.
const PAINTED_FLOOR: usize = (WIDTH as usize * HEIGHT as usize) / 2;

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn docker_status(args: &[&str]) -> (bool, String) {
    let output = Command::new("docker")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("could not run `docker {}`: {e}", args.join(" ")));
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text.trim().to_string())
}

/// Run docker with the full output printed: it is part of the evidence.
fn docker(args: &[&str]) -> String {
    let (ok, text) = docker_status(args);
    println!("$ docker {}\n{text}", args.join(" "));
    assert!(ok, "`docker {}` failed", args.join(" "));
    text
}

/// A host port nothing is listening on, for the published container port.
fn free_port() -> u16 {
    let listener = TcpListener::bind((HOST, 0)).expect("a free host port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

/// Build and start the xrdp fixture, returning the host port 3389 is published
/// on. A container left behind by an earlier run is replaced.
fn start_fixture() -> u16 {
    let repo = repo_root();
    let fixture_dir = repo.join("deploy/xrdp-host");
    docker(&[
        "build",
        "-t",
        FIXTURE_IMAGE,
        "-f",
        &fixture_dir.join("Dockerfile").display().to_string(),
        &fixture_dir.display().to_string(),
    ]);
    let _ = docker_status(&["rm", "-f", CONTAINER]);
    let port = free_port();
    docker(&[
        "run",
        "-d",
        "--name",
        CONTAINER,
        // Xorg and Xfce want more shared memory than the 64 MiB default.
        "--shm-size=512m",
        "-p",
        &format!("{HOST}:{port}:{CONTAINER_PORT}"),
        FIXTURE_IMAGE,
    ]);
    println!("xrdp fixture {CONTAINER} published on {HOST}:{port}");
    port
}

/// Whether xrdp has logged that it started listening. A TCP connect to the
/// *published* port is not this: Docker's port forwarder accepts a host
/// connection before anything inside the container is listening, so the connect
/// succeeds against a server that is not up yet — which is exactly how the cold
/// run of this test failed, with xrdp's own log showing no connection received
/// at all and the client reading a frame from a socket the forwarder closed.
fn xrdp_reports_listening() -> bool {
    let (ok, log) = docker_status(&["exec", CONTAINER, "cat", "/var/log/xrdp.log"]);
    ok && log.contains("listening to port")
}

/// Poll until xrdp is serving: on a container this test started, xrdp says so
/// itself, and then the published host port answers. Readiness only — nothing
/// here is the frame test.
fn wait_for_xrdp(port: u16, owned: bool) {
    let deadline = Instant::now() + READY_DEADLINE;
    let addr = format!("{HOST}:{port}").parse().expect("a host:port pair");
    let mut last = String::from("xrdp had not started listening yet");
    loop {
        let listening = !owned || xrdp_reports_listening();
        if listening {
            match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
                Ok(_) => {
                    println!("xrdp is accepting connections on {HOST}:{port}");
                    return;
                }
                Err(e) => last = e.to_string(),
            }
        }
        if Instant::now() > deadline {
            panic!(
                "xrdp never accepted a connection on {HOST}:{port} in {READY_DEADLINE:?}: {last}"
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Print what the server said, so a failure is diagnosable from the run alone.
fn report_server_logs() {
    for log in ["/var/log/xrdp.log", "/var/log/xrdp-sesman.log"] {
        let (_, text) = docker_status(&["exec", CONTAINER, "cat", log]);
        println!("$ docker exec {CONTAINER} cat {log}\n{text}");
    }
    let (_, text) = docker_status(&["exec", CONTAINER, "sh", "-c", "ps aux | head -20"]);
    println!("$ docker exec {CONTAINER} ps aux | head -20\n{text}");
    let (_, text) = docker_status(&["logs", "--tail", "40", CONTAINER]);
    println!("$ docker logs --tail 40 {CONTAINER}\n{text}");
}

/// Send the SDK's `log` lines and the IronRDP engine's `tracing` events to the
/// test's own output. Without both installed, why a session dropped never
/// reaches the run at all.
fn init_logging() {
    let level = std::env::var("GOBLE_RDP_E2E_LOG").unwrap_or_else(|_| "warn".to_string());
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&level))
        .is_test(false)
        .try_init();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&level)),
        )
        .with_target(true)
        .try_init();
}

/// Pixels that are not the placeholder's zero, the number of distinct colours
/// among them, and a sample that makes a frame recognisable in the run's output.
fn painted(frame: &goble_screen_core::ScreenFrame) -> (usize, usize, String) {
    let pixels: Vec<[u8; 4]> = frame
        .data
        .chunks_exact(4)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect();
    let non_black = pixels
        .iter()
        .filter(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
        .count();
    let mut colours: Vec<[u8; 4]> = pixels
        .iter()
        .copied()
        .filter(|px| *px != [0, 0, 0, 0])
        .collect();
    colours.sort_unstable();
    colours.dedup();
    let sample: Vec<String> = colours
        .iter()
        .take(3)
        .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
        .collect();
    (non_black, colours.len(), sample.join(" "))
}

#[test]
#[ignore = "needs the Docker daemon: connects to the xrdp fixture in deploy/xrdp-host, starting it when GOBLE_RDP_E2E_PORT is unset"]
fn a_real_xrdp_session_decodes_a_frame_at_the_desktop_size() {
    init_logging();

    let owned = std::env::var("GOBLE_RDP_E2E_PORT").is_err();
    let port: u16 = match std::env::var("GOBLE_RDP_E2E_PORT") {
        Ok(value) => value.parse().expect("GOBLE_RDP_E2E_PORT is a port number"),
        Err(_) => start_fixture(),
    };
    wait_for_xrdp(port, owned);

    // The real source, registered into a real registry exactly as the app wires
    // it: `connect` spawns the IronRDP client and registers the capturer alone.
    let registry = ScreenRegistry::new();
    let mut config = RemoteConfig::new(HOST, USERNAME, PASSWORD);
    config.port = port;
    config.width = WIDTH;
    config.height = HEIGHT;
    let source =
        RdpRemoteSource::connect(SOURCE, config, &registry).expect("the RDP source connects");
    assert_eq!(
        registry.capturer_sources(),
        vec![SOURCE.to_string()],
        "connect registers the capturer under its source"
    );
    assert!(
        registry.controller_sources().is_empty(),
        "a freshly opened desktop is view-only: the controller is the caller's"
    );
    assert!(!registry.has_control(SOURCE));
    // Taking the desktop registers the controller the caller holds; releasing
    // puts the source back to view-only with its capturer intact.
    registry
        .take_control(SOURCE, ControlHolder::User, Arc::clone(&source.controller))
        .expect("the user takes the desktop the source exposed");
    assert_eq!(registry.control_holder(SOURCE), Some(ControlHolder::User));
    registry
        .release_control(SOURCE, ControlHolder::User)
        .expect("the user releases it");
    assert!(
        registry.controller_sources().is_empty(),
        "releasing leaves nothing driving"
    );

    // Poll the capture path until the session has painted a desktop. Every
    // value in this loop before that is the placeholder: the dimensions of the
    // requested desktop, all-zero pixels.
    let deadline = Instant::now() + FRAME_DEADLINE;
    let frame = loop {
        let frame = registry
            .capture(SOURCE)
            .expect("the registry resolves the source it was given");
        let (non_black, _, _) = painted(&frame);
        if non_black > PAINTED_FLOOR {
            break frame;
        }
        if Instant::now() > deadline {
            println!("the capture path held only the blank placeholder for {FRAME_DEADLINE:?}");
            report_server_logs();
            panic!(
                "no decoded desktop arrived from {HOST}:{port} in {FRAME_DEADLINE:?}: every \
                 capture was the {WIDTH}x{HEIGHT} placeholder, all-zero, or under the \
                 {PAINTED_FLOOR}-pixel floor. The reason the session ended is in the \
                 `log`/`tracing` lines above; the T4 entry of \
                 `.agents/10-platform-and-performance/testing-and-ci.md` records what this \
                 fixture answered"
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    };

    let (non_black, colours, sample) = painted(&frame);
    println!(
        "decoded frame: {}x{} ({} bytes, {} of {} pixels painted in {colours} colours, \
         sample {sample})",
        frame.width,
        frame.height,
        frame.len(),
        non_black,
        frame.width as usize * frame.height as usize,
    );

    // A decoded frame: the desktop's buffer, non-zero, at the dimensions the
    // client requested and the server's session carries.
    assert!(
        !frame.is_empty(),
        "the frame carries no pixels at all: {} bytes",
        frame.len()
    );
    assert_eq!(
        (frame.width, frame.height),
        (WIDTH as u32, HEIGHT as u32),
        "the decoded frame is not the desktop the client asked for"
    );
    assert_eq!(
        frame.len(),
        WIDTH as usize * HEIGHT as usize * 4,
        "the frame is not an RGBA8 buffer of the desktop"
    );
    assert_eq!(frame.source, SOURCE, "the frame names another source");
    assert!(
        non_black > 0,
        "every pixel is zero, which is the placeholder this test exists to tell \
         apart from a decoded frame"
    );
    assert!(
        non_black > PAINTED_FLOOR,
        "only {non_black} pixels differ from the placeholder: that is not a painted desktop"
    );

    if owned {
        docker(&["rm", "-f", CONTAINER]);
    }
}
