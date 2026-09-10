//! Screen domain state: broadcast + computer-use flags, the capturable source
//! list, and a deterministic screen-event recorder/replayer.
//!
//! Mirrors the plain [`crate::ui::ScreenSnapshot`] but lives in the executable
//! so it is owned by the app. Broadcast (`live capture`) and computer-use
//! (`click/type/scroll`) are toggled by the user; the source list is read live
//! from the backend's [`goble_screen_core::ScreenRegistry`] (exposed through
//! [`goble_desktop_service::DesktopState::screen_registry`]), which is seeded
//! with the local platform adapter on construction.
//!
//! The recorder captures *observation* events (broadcast frame grabs) and can
//! accept *input* events (click/type/scroll), then replays them through the
//! same screen registry deterministically (record start/stop, replay once or
//! loop, timing). This is a screen-surface feature, distinct from the
//! agent-controlled conversation rewind/replay in `goble-replay`.

use std::time::Instant;

use goble_desktop_service::DesktopState;
use goble_screen_core::ScreenFrame;

use crate::ui::ScreenSourceEntry;

/// The source id used by the [`ScreenState::mock`] test fixture.
const MOCK_SOURCE: &str = "local";

/// A recorded screen event, timestamped in milliseconds relative to when
/// recording started. Order is preserved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenRecordEvent {
    /// Milliseconds since the recording started (or relative to replay start).
    pub at_ms: u64,
    pub kind: ScreenRecordKind,
}

/// The two kinds of screen event the recorder captures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenRecordKind {
    /// An observation: a captured frame (its dimensions + source).
    Capture { source: String, width: u32, height: u32 },
    /// A controller input event.
    Click { x: u32, y: u32 },
    TypeText { text: String },
    Scroll { dx: i32, dy: i32 },
}

impl ScreenRecordEvent {
    fn new(at_ms: u64, kind: ScreenRecordKind) -> Self {
        Self { at_ms, kind }
    }
}

#[derive(Clone)]
pub struct ScreenState {
    /// Whether the screen sheet is open.
    pub open: bool,
    /// Live capture: whether broadcast is streaming the selected source.
    pub broadcast: bool,
    /// Computer-use: whether click/type/scroll input is enabled.
    pub computer_use: bool,
    /// The source the panel is operating on.
    pub selected_source: String,
    /// The capturable/controllable sources from the screen registry.
    pub sources: Vec<ScreenSourceEntry>,
    /// Short status of the last broadcast capture (dimensions or an error).
    pub last_capture: Option<String>,
    /// Whether the recorder is actively capturing events.
    pub recording: bool,
    /// Whether a replay is currently scheduled to emit recorded events.
    pub replaying: bool,
    /// Whether the replay loops (repeats from the start) or runs once.
    pub replay_loop: bool,
    /// When recording started, for event timestamps.
    pub recording_started: Option<Instant>,
    /// The events recorded so far, in order.
    pub recorded: Vec<ScreenRecordEvent>,
    /// Index of the next recorded event to emit during replay.
    pub replay_index: usize,
    /// Elapsed replay time (ms) accumulated by [`ScreenState::step_replay`].
    pub replay_elapsed_ms: u64,
    /// When the last replay tick happened, so a wall-clock tick can advance it.
    pub replay_last_tick: Option<Instant>,
    /// Non-empty status describing the last replay step (e.g. an input error).
    pub replay_status: Option<String>,
    /// The most recently captured frame for `selected_source`, shown live when
    /// broadcast is on. `None` until a capture succeeds (or broadcast is off).
    pub frame: Option<ScreenFrame>,
    /// Monotonic parity for the held frame, so [`crate::ui::FrameView`] can
    /// detect pixel changes and skip re-uploads of unchanged frames.
    pub frame_seq: u64,
}

impl ScreenState {
    /// Start from real backend data, populating the source list from the
    /// screen registry. Broadcast and computer-use start disabled.
    pub fn from_desktop(desktop: &DesktopState) -> Self {
        let mut state = Self {
            open: false,
            broadcast: false,
            computer_use: false,
            selected_source: String::new(),
            sources: Vec::new(),
            last_capture: None,
            recording: false,
            replaying: false,
            replay_loop: false,
            recording_started: None,
            recorded: Vec::new(),
            replay_index: 0,
            replay_elapsed_ms: 0,
            replay_last_tick: None,
            replay_status: None,
            frame: None,
            frame_seq: 0,
        };
        state.refresh_sources(desktop);
        state
    }

    /// Reload the capturable/controllable source list from the screen registry.
    pub fn refresh_sources(&mut self, desktop: &DesktopState) {
        let registry = desktop.screen_registry();
        let capturers = registry.capturer_sources();
        let controllers = registry.controller_sources();

        let mut sources: Vec<ScreenSourceEntry> = capturers
            .iter()
            .map(|source| ScreenSourceEntry {
                source: source.clone(),
                capturable: true,
                controllable: controllers.contains(source),
            })
            .collect();
        // Controller-only sources (unusual but complete): keep them visible.
        for source in &controllers {
            if !capturers.contains(source) {
                sources.push(ScreenSourceEntry {
                    source: source.clone(),
                    capturable: false,
                    controllable: true,
                });
            }
        }

        self.sources = sources;

        if !self
            .sources
            .iter()
            .any(|s| s.source == self.selected_source)
        {
            // No sources: the selection is cleared rather than substituting a
            // synthetic source (the registry is seeded with the local adapter).
            self.selected_source = self
                .sources
                .first()
                .map(|s| s.source.clone())
                .unwrap_or_default();
        }
    }

    /// Select a source; an unknown id is ignored.
    pub fn select_source(&mut self, source: &str) {
        if self.sources.iter().any(|s| s.source == source) {
            self.selected_source = source.to_string();
        }
    }

    /// Toggle live capture. Turning it on attempts a best-effort grab through
    /// the screen registry; turning it off clears the status and held frame.
    pub fn toggle_broadcast(&mut self, on: bool, desktop: Option<&DesktopState>) {
        self.broadcast = on;
        if on {
            self.poll_capture(desktop);
        } else {
            self.last_capture = None;
            self.frame = None;
        }
    }

    /// Best-effort grab of `selected_source` and hold it as the live frame,
    /// updating the status line. A successful grab is recorded as an
    /// observation event while recording. Called by `toggle_broadcast` and by
    /// the per-frame `tick` while the sheet is open and broadcasting.
    fn poll_capture(&mut self, desktop: Option<&DesktopState>) {
        let Some(desktop) = desktop else {
            // No backend: nothing to capture (the app always has a store), so
            // leave the status/frame untouched rather than faking a broadcast.
            return;
        };
        let source = self.selected_source.clone();
        match desktop.screen_registry().capture(&source) {
            Ok(frame) => {
                self.frame_seq += 1;
                if self.recording {
                    // Record the observation (dimensions + source).
                    let at_ms = self.elapsed_recording_ms();
                    self.recorded.push(ScreenRecordEvent::new(
                        at_ms,
                        ScreenRecordKind::Capture {
                            source: frame.source.clone(),
                            width: frame.width,
                            height: frame.height,
                        },
                    ));
                }
                self.last_capture =
                    Some(format!("{}×{} @ {}", frame.width, frame.height, frame.source));
                self.frame = Some(frame);
            }
            Err(e) => self.last_capture = Some(format!("capture failed: {e}")),
        }
    }

    /// Toggle computer-use (click/type/scroll) input.
    pub fn toggle_computer_use(&mut self, on: bool) {
        self.computer_use = on;
    }

    /// A pointer click at source pixel `(x, y)` on the selected source, routed
    /// through the registry controller when computer-use is on. The event is
    /// also recorded (for a later replay) regardless of computer-use.
    pub fn click(&mut self, x: u32, y: u32, desktop: Option<&DesktopState>) {
        self.record_input(ScreenRecordKind::Click { x, y });
        if !self.computer_use {
            return;
        }
        if let Some(desktop) = desktop {
            let source = self.selected_source.clone();
            if let Err(e) = desktop.screen_registry().click(&source, x, y) {
                self.replay_status = Some(format!("click failed: {e}"));
            }
        }
    }

    /// Type `text` into the focused field of the selected source, routed
    /// through the registry controller when computer-use is on.
    pub fn type_text(&mut self, text: &str, desktop: Option<&DesktopState>) {
        self.record_input(ScreenRecordKind::TypeText { text: text.to_string() });
        if !self.computer_use {
            return;
        }
        if let Some(desktop) = desktop {
            let source = self.selected_source.clone();
            if let Err(e) = desktop.screen_registry().type_text(&source, text) {
                self.replay_status = Some(format!("type failed: {e}"));
            }
        }
    }

    /// Scroll the selected source by `(dx, dy)` ticks, routed through the
    /// registry controller when computer-use is on.
    pub fn scroll(&mut self, dx: i32, dy: i32, desktop: Option<&DesktopState>) {
        self.record_input(ScreenRecordKind::Scroll { dx, dy });
        if !self.computer_use {
            return;
        }
        if let Some(desktop) = desktop {
            let source = self.selected_source.clone();
            if let Err(e) = desktop.screen_registry().scroll(&source, dx, dy) {
                self.replay_status = Some(format!("scroll failed: {e}"));
            }
        }
    }

    /// Begin recording screen events. Clears any prior recording.
    pub fn start_recording(&mut self) {
        self.recording = true;
        self.recorded.clear();
        self.recording_started = Some(Instant::now());
        self.stop_replay();
        self.last_capture = Some("recording…".to_string());
    }

    /// Stop recording, leaving the recorded events in place for replay.
    pub fn stop_recording(&mut self) {
        self.recording = false;
        self.recording_started = None;
        if !self.replaying {
            self.last_capture = match self.recorded.len() {
                0 => Some("(nothing recorded)".to_string()),
                n => Some(format!("recorded {n} event(s)")),
            };
        }
    }

    /// Record an input event (click/type/scroll) if currently recording.
    pub fn record_input(&mut self, kind: ScreenRecordKind) {
        if !self.recording {
            return;
        }
        let at_ms = self.elapsed_recording_ms();
        self.recorded.push(ScreenRecordEvent::new(at_ms, kind));
    }

    /// Milliseconds elapsed since recording began.
    fn elapsed_recording_ms(&self) -> u64 {
        self.recording_started
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    /// Begin replaying the recorded events, once (or looping when `loop_`).
    /// `Instant` is only used to let a wall-clock tick advance the schedule; the
    /// deterministic advance comes from [`ScreenState::step_replay`].
    pub fn start_replay(&mut self, loop_: bool) {
        if self.recorded.is_empty() {
            self.replay_status = Some("nothing recorded to replay".to_string());
            return;
        }
        self.replaying = true;
        self.replay_loop = loop_;
        self.replay_index = 0;
        self.replay_elapsed_ms = 0;
        self.replay_last_tick = Some(Instant::now());
        self.replay_status = Some("replaying…".to_string());
        self.recording = false;
    }

    /// Stop a replay, leaving the recording intact so it can be replayed again.
    pub fn stop_replay(&mut self) {
        if self.replaying {
            self.replay_status = Some(format!(
                "replay stopped ({}/{} events)",
                self.replay_index,
                self.recorded.len()
            ));
        }
        self.replaying = false;
        self.replay_last_tick = None;
    }

    /// Advance the replay deterministically by `advance_ms`, emitting every
    /// recorded event whose `at_ms` is now due. This is the testable primitive:
    /// it never sleeps and never consults a clock for the schedule. When an
    /// input event is emitted it is dispatched through the backend's screen
    /// registry controller for the selected source; observations update the
    /// status. Returns the number of events emitted this step.
    pub fn step_replay(&mut self, advance_ms: u64, desktop: Option<&DesktopState>) -> usize {
        if !self.replaying {
            return 0;
        }
        self.replay_elapsed_ms += advance_ms;
        let mut emitted = 0;
        let source = self.selected_source.clone();
        while let Some(event) = self.recorded.get(self.replay_index) {
            if event.at_ms > self.replay_elapsed_ms {
                break;
            }
            match &event.kind {
                ScreenRecordKind::Capture {
                    source: s,
                    width,
                    height,
                } => {
                    self.last_capture =
                        Some(format!("replay {}×{} @ {}", width, height, s));
                }
                ScreenRecordKind::Click { x, y } => {
                    if let Some(desktop) = desktop {
                        match desktop.screen_registry().click(&source, *x, *y) {
                            Ok(()) => {}
                            Err(e) => self.replay_status = Some(format!("replay click failed: {e}")),
                        }
                    }
                }
                ScreenRecordKind::TypeText { text } => {
                    if let Some(desktop) = desktop {
                        match desktop.screen_registry().type_text(&source, text) {
                            Ok(()) => {}
                            Err(e) => self.replay_status = Some(format!("replay type failed: {e}")),
                        }
                    }
                }
                ScreenRecordKind::Scroll { dx, dy } => {
                    if let Some(desktop) = desktop {
                        match desktop.screen_registry().scroll(&source, *dx, *dy) {
                            Ok(()) => {}
                            Err(e) => self.replay_status = Some(format!("replay scroll failed: {e}")),
                        }
                    }
                }
            }
            self.replay_index += 1;
            emitted += 1;
        }

        if self.replay_index >= self.recorded.len() {
            if self.replay_loop {
                self.replay_index = 0;
                self.replay_elapsed_ms = 0;
            } else {
                self.stop_replay();
            }
        }
        emitted
    }

    /// Wall-clock tick called once per frame from the root view. While the
    /// sheet is open and broadcasting, refreshes the held live frame; then
    /// advances the replay schedule by the time elapsed since the last tick so
    /// a running replay plays back in (roughly) real time.
    pub fn tick(&mut self, desktop: Option<&DesktopState>) {
        if self.open && self.broadcast {
            self.poll_capture(desktop);
        }
        if !self.replaying {
            return;
        }
        let now = Instant::now();
        let dt_ms = self
            .replay_last_tick
            .map(|last| now.saturating_duration_since(last).as_millis() as u64)
            .unwrap_or(0);
        self.replay_last_tick = Some(now);
        self.step_replay(dt_ms, desktop);
    }

    /// Test fixture: a single local source ready to broadcast. Used only by
    /// tests; the app always builds from the real screen registry via
    /// [`Self::from_desktop`].
    pub fn mock() -> Self {
        Self {
            open: false,
            broadcast: false,
            computer_use: false,
            selected_source: MOCK_SOURCE.to_string(),
            sources: vec![ScreenSourceEntry {
                source: MOCK_SOURCE.to_string(),
                capturable: true,
                controllable: true,
            }],
            last_capture: None,
            recording: false,
            replaying: false,
            replay_loop: false,
            recording_started: None,
            recorded: Vec::new(),
            replay_index: 0,
            replay_elapsed_ms: 0,
            replay_last_tick: None,
            replay_status: None,
            frame: None,
            frame_seq: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ScreenSourceEntry;

    fn entry(source: &str) -> ScreenSourceEntry {
        ScreenSourceEntry {
            source: source.to_string(),
            capturable: true,
            controllable: false,
        }
    }

    #[test]
    fn mock_exposes_single_local_source() {
        let state = ScreenState::mock();
        assert!(!state.open);
        assert!(!state.broadcast);
        assert!(!state.computer_use);
        assert_eq!(state.selected_source, "local");
        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.sources[0].source, "local");
        assert!(state.sources[0].capturable);
        assert!(state.sources[0].controllable);
    }

    #[test]
    fn select_source_ignores_unknown_id() {
        let mut state = ScreenState::mock();
        state.select_source("not-a-source");
        assert_eq!(state.selected_source, "local");
    }

    #[test]
    fn select_source_updates_selection() {
        let mut state = ScreenState::mock();
        state.sources.push(entry("display:1"));
        state.select_source("display:1");
        assert_eq!(state.selected_source, "display:1");
    }

    #[test]
    fn toggling_computer_use_flips_flag() {
        let mut state = ScreenState::mock();
        state.toggle_computer_use(true);
        assert!(state.computer_use);
        state.toggle_computer_use(false);
        assert!(!state.computer_use);
    }

    #[test]
    fn broadcast_without_backend_keeps_no_fake_status() {
        let mut state = ScreenState::mock();
        state.toggle_broadcast(true, None);
        assert!(state.broadcast);
        // No backend to capture from, so no fabricated status is reported.
        assert_eq!(state.last_capture, None);
        state.toggle_broadcast(false, None);
        assert!(!state.broadcast);
        assert_eq!(state.last_capture, None);
    }

    #[test]
    fn refresh_sources_populates_from_registry() {
        let dir = tempfile::tempdir().expect("create temp thread store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("open in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("open thread store"),
        );
        let mut state = ScreenState::from_desktop(&desktop);
        assert!(!state.sources.is_empty(), "registry has a local source");
        assert_eq!(state.selected_source, state.sources[0].source);
        // Refresh is idempotent and keeps the current selection.
        let before = state.selected_source.clone();
        state.refresh_sources(&desktop);
        assert_eq!(state.selected_source, before);
    }

    #[test]
    fn recording_broadcast_captures_observation_events() {
        let mut state = ScreenState::mock();
        state.start_recording();
        assert!(state.recording);
        state.toggle_broadcast(true, None);
        state.toggle_broadcast(false, None);
        state.stop_recording();
        assert!(!state.recording);
        // No backend, so no frame is actually captured -> no observation events.
        assert!(state.recorded.is_empty());
    }

    #[test]
    fn record_input_appends_typed_event() {
        let mut state = ScreenState::mock();
        state.start_recording();
        state.record_input(ScreenRecordKind::TypeText { text: "hi".into() });
        state.stop_recording();
        assert_eq!(state.recorded.len(), 1);
        assert!(matches!(
            state.recorded[0].kind,
            ScreenRecordKind::TypeText { ref text } if text == "hi"
        ));
    }

    #[test]
    fn replay_emits_events_once_in_timing_order() {
        let mut state = ScreenState::mock();
        state.start_recording();
        state
            .recorded
            .push(ScreenRecordEvent::new(0, ScreenRecordKind::Capture {
                source: "local".into(),
                width: 10,
                height: 20,
            }));
        state
            .recorded
            .push(ScreenRecordEvent::new(50, ScreenRecordKind::Capture {
                source: "local".into(),
                width: 30,
                height: 40,
            }));
        state.stop_recording();

        state.start_replay(false);
        assert!(state.replaying);
        // At t=10 only the first event (at 0ms) is due.
        let emitted = state.step_replay(10, None);
        assert_eq!(emitted, 1);
        assert_eq!(state.replay_index, 1);
        assert_eq!(state.last_capture.as_deref(), Some("replay 10×20 @ local"));
        // At t=60 the second event (at 50ms) is due and the replay finishes.
        let emitted = state.step_replay(50, None);
        assert_eq!(emitted, 1);
        assert!(!state.replaying, "once-replay stops after the last event");
        assert_eq!(state.replay_index, 2);
    }

    #[test]
    fn replay_loop_restarts_from_the_beginning() {
        let mut state = ScreenState::mock();
        state.start_recording();
        state
            .recorded
            .push(ScreenRecordEvent::new(0, ScreenRecordKind::Capture {
                source: "local".into(),
                width: 1,
                height: 2,
            }));
        state.stop_recording();

        state.start_replay(true);
        assert!(state.replay_loop);
        let _ = state.step_replay(0, None);
        assert_eq!(state.replay_index, 0, "loop resets after emitting all");
        assert!(state.replaying);
    }

    #[test]
    fn replay_with_no_recording_reports_status() {
        let mut state = ScreenState::mock();
        state.start_replay(false);
        assert!(!state.replaying);
        assert_eq!(
            state.replay_status.as_deref(),
            Some("nothing recorded to replay")
        );
    }

    /// A DesktopState whose local capturer is a deterministic mock, so capture
    /// yields a known frame without a live screen backend.
    fn desktop_with_mock_capturer() -> (std::sync::Arc<DesktopState>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp thread store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("open in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("open thread store"),
        );
        let frame = goble_screen_core::ScreenFrame::new("local", 2, 3, vec![9u8; 2 * 3 * 4]);
        desktop.screen_registry().register_capturer(std::sync::Arc::new(
            goble_screen_core::MockCapturer::new("local").with_frame(frame),
        ));
        (desktop, dir)
    }

    #[test]
    fn broadcast_captures_and_holds_live_frame() {
        let (desktop, _dir) = desktop_with_mock_capturer();
        let mut state = ScreenState::from_desktop(&desktop);
        assert!(state.frame.is_none());
        state.toggle_broadcast(true, Some(&desktop));
        assert!(state.broadcast);
        let frame = state.frame.as_ref().expect("a frame is held");
        assert_eq!(frame.source, "local");
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 3);
        assert_eq!(state.frame_seq, 1);
        // Turning broadcast off clears the held frame.
        state.toggle_broadcast(false, Some(&desktop));
        assert!(state.frame.is_none());
    }

    #[test]
    fn tick_polls_frame_while_open_and_broadcasting() {
        let (desktop, _dir) = desktop_with_mock_capturer();
        let mut state = ScreenState::from_desktop(&desktop);
        state.open = true;
        state.toggle_broadcast(true, Some(&desktop));
        state.frame = None;
        state.tick(Some(&desktop));
        assert!(state.frame.is_some(), "tick should re-capture while open+broadcast");
    }

    #[test]
    fn input_routes_to_controller_when_computer_use_on() {
        let (desktop, _dir) = desktop_with_mock_capturer();
        let ctl = std::sync::Arc::new(goble_screen_core::MockController::new("local"));
        desktop.screen_registry().register_controller(ctl.clone());

        let mut state = ScreenState::from_desktop(&desktop);
        assert!(!state.computer_use);
        state.toggle_computer_use(true);
        state.click(10, 20, Some(&desktop));
        state.type_text("hi", Some(&desktop));
        state.scroll(-1, 2, Some(&desktop));

        assert_eq!(
            ctl.actions(),
            vec![
                goble_screen_core::MockAction::Click { x: 10, y: 20 },
                goble_screen_core::MockAction::TypeText { text: "hi".into() },
                goble_screen_core::MockAction::Scroll { dx: -1, dy: 2 },
            ]
        );
    }

    #[test]
    fn input_is_ignored_when_computer_use_off() {
        let (desktop, _dir) = desktop_with_mock_capturer();
        let ctl = std::sync::Arc::new(goble_screen_core::MockController::new("local"));
        desktop.screen_registry().register_controller(ctl.clone());

        let mut state = ScreenState::from_desktop(&desktop);
        assert!(!state.computer_use);
        state.click(10, 20, Some(&desktop));
        state.type_text("hi", Some(&desktop));
        assert!(
            ctl.actions().is_empty(),
            "no input should reach the controller when computer-use is off"
        );
    }
}
