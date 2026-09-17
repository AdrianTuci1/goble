use goble_core::agent::Trigger;
use goble_core::protocol::WorkerMessage;
use goble_core::store::Store;
use goble_core::workflow::WorkflowStep;
use goble_screen_core::ControlHolder;

use crate::state::*;

use super::tmp_state;

#[test]
fn test_state_add_worker() {
    let (_dir, state) = tmp_state();
    let wid = WorkerId::generate();
    state
        .add_worker(
            wid.clone(),
            "vps".to_string(),
            "wss://localhost:8787/ws".to_string(),
        )
        .unwrap();
    let workers = state.list_workers();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].name, "vps");
    assert!(!workers[0].paired);
    state.remove_worker(&wid);
    assert!(state.list_workers().is_empty());
}

#[test]
fn screen_registry_has_local_capturer_and_controller() {
    let (_dir, state) = tmp_state();
    let reg = state.screen_registry();
    assert!(
        reg.capturer(goble_screen_adapter::LOCAL_SOURCE).is_some(),
        "a local capturer must be registered on DesktopState"
    );
    assert!(
        reg.controller(goble_screen_adapter::LOCAL_SOURCE).is_some(),
        "a local controller must be registered on DesktopState"
    );
    assert_eq!(
        reg.capturer_sources(),
        vec![goble_screen_adapter::LOCAL_SOURCE.to_string()]
    );
}

#[test]
#[cfg(not(feature = "remote-screen"))]
fn open_remote_screen_without_feature_errors() {
    let (_dir, state) = tmp_state();
    let err = state
        .open_remote_screen(goble_harness_types::RemoteScreenConfig::new(
            "vm",
            "desktop-account",
        ))
        .unwrap_err();
    assert!(
        err.to_string().contains("remote-screen"),
        "the error should name the missing feature, got: {err}"
    );
}

/// The handoff names a credential and this is where its value is resolved: the
/// stored value is the account line, and the password never leaves the caller
/// that builds the RDP connection.
#[test]
fn a_stored_credential_resolves_to_the_desktop_account() {
    let (_dir, state) = tmp_state();
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    let (username, password) = state.resolve_screen_credential("desktop-account").unwrap();
    assert_eq!(username, "goble");
    assert_eq!(password, "hunter2");
}

/// A handoff with nothing to resolve fails loudly instead of opening a desktop
/// with no account: an unstored name, a handoff that named none, and a value
/// that is not an account line are each refused, and none of the errors carries
/// the value it refused.
#[test]
fn an_unresolvable_credential_fails_loudly_without_the_value() {
    let (_dir, state) = tmp_state();

    let err = state
        .resolve_screen_credential("desktop-account")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("no stored credential named `desktop-account`"),
        "{err}"
    );

    let err = state.resolve_screen_credential("   ").unwrap_err();
    assert!(err.to_string().contains("named no credential"), "{err}");

    state.set_credential("bare", "hunter2").unwrap();
    let err = state.resolve_screen_credential("bare").unwrap_err();
    assert!(
        err.to_string().contains("account line `username:password`"),
        "{err}"
    );
    assert!(
        !err.to_string().contains("hunter2"),
        "the refused value must not be echoed: {err}"
    );

    state.set_credential("half", "goble:").unwrap();
    let err = state.resolve_screen_credential("half").unwrap_err();
    assert!(
        err.to_string().contains("incomplete desktop account"),
        "{err}"
    );
}

#[test]
#[cfg(feature = "remote-screen")]
fn open_remote_screen_registers_a_view_only_source() {
    // The client connects lazily on its own thread; registering the capturer is
    // synchronous, so a dead endpoint still yields a registered (blank) source.
    // A connection-refused endpoint fails fast.
    let (_dir, state) = tmp_state();
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    let source = state
        .open_remote_screen(goble_harness_types::RemoteScreenConfig::new(
            "127.0.0.1",
            "desktop-account",
        ))
        .unwrap();
    let reg = state.screen_registry();
    assert!(
        reg.capturer(&source).is_some(),
        "remote capturer registered"
    );
    // The desktop is view-only until it is taken: the agent that asked for it
    // and the user watching it can both reach it, and only one may drive.
    assert!(
        reg.controller(&source).is_none(),
        "opening a remote desktop must register no writer"
    );
    assert!(!reg.has_control(&source));
    assert!(matches!(
        reg.click(&source, 1, 2)
            .unwrap_err()
            .downcast_ref::<goble_screen_core::ScreenError>(),
        Some(goble_screen_core::ScreenError::NoController(s)) if s == &source
    ));

    // The user takes it: the controller the host kept is registered, and the
    // agent is refused while the user drives — that refusal is the agent's ask.
    state
        .take_screen_control(&source, ControlHolder::User)
        .unwrap();
    assert!(reg.controller(&source).is_some());
    assert_eq!(reg.control_holder(&source), Some(ControlHolder::User));
    let err = state
        .take_screen_control(&source, ControlHolder::Agent)
        .unwrap_err();
    assert!(err.to_string().contains("driven by the user"), "{err}");
    assert_eq!(reg.control_holder(&source), Some(ControlHolder::User));

    // The user releases, and the refused ask succeeds.
    state
        .release_screen_control(&source, ControlHolder::User)
        .unwrap();
    assert!(!reg.has_control(&source));
    state
        .take_screen_control(&source, ControlHolder::Agent)
        .unwrap();
    assert_eq!(reg.control_holder(&source), Some(ControlHolder::Agent));
}

/// The open path resolves before it connects: a handoff whose credential is not
/// stored fails at open time and registers no source, rather than connecting
/// with an empty account.
#[test]
#[cfg(feature = "remote-screen")]
fn open_remote_screen_without_a_stored_credential_fails() {
    let (_dir, state) = tmp_state();
    let err = state
        .open_remote_screen(goble_harness_types::RemoteScreenConfig::new(
            "127.0.0.1",
            "not-stored",
        ))
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("no stored credential named `not-stored`"),
        "{err}"
    );
    assert!(
        state
            .screen_registry()
            .capturer_sources()
            .iter()
            .all(|source| !source.starts_with("remote-xrdp:")),
        "a failed handoff registers no source: {:?}",
        state.screen_registry().capturer_sources()
    );
}

/// Closing a remote desktop is `unregister_source` plus the parked controller:
/// the source leaves the registry (capturer and controller alike) and whoever
/// held it holds nothing, because the entry was the hold. The close is
/// announced so the app stops drawing it.
#[test]
fn closing_a_remote_desktop_removes_the_source_and_its_controller() {
    let (dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let source = "remote-xrdp:vm:3389";
    let reg = state.screen_registry();
    reg.register_capturer(Arc::new(
        goble_screen_core::MockCapturer::new(source).with_blank(2, 2),
    ));
    reg.register_controller(Arc::new(goble_screen_core::MockController::new(source)));
    assert_eq!(reg.capture(source).unwrap().len(), 16);

    assert!(
        state.close_remote_screen(source),
        "the source was registered"
    );
    assert!(
        reg.capturer(source).is_none() && reg.controller(source).is_none(),
        "both halves left with the source"
    );
    assert!(
        !reg.capturer_sources().contains(&source.to_string()),
        "the registry no longer lists it: {:?}",
        reg.capturer_sources()
    );
    assert!(!reg.has_control(source), "the writer went with the entry");
    let events = bus.take_events();
    assert!(
        events.iter().any(|(name, payload)| name == "screen:closed"
            && payload.get("source").and_then(|v| v.as_str()) == Some(source)),
        "the close is announced with its source: {events:?}"
    );

    // Closing it again, or closing one that was never opened, changes nothing.
    assert!(!state.close_remote_screen(source));
    assert!(!state.close_remote_screen("remote-xrdp:other:3389"));
    drop(dir);
}

/// The close path reaches a source `open_remote_screen` registered, and the
/// controller parked for it is dropped with the source — so nothing is left to
/// take afterwards.
#[test]
#[cfg(feature = "remote-screen")]
fn closing_a_remote_desktop_opened_by_the_host_unregisters_it() {
    let (_dir, state) = tmp_state();
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    let source = state
        .open_remote_screen(goble_harness_types::RemoteScreenConfig::new(
            "127.0.0.1",
            "desktop-account",
        ))
        .unwrap();
    let reg = state.screen_registry();
    assert!(reg.capturer(&source).is_some());

    assert!(state.close_remote_screen(&source));
    assert!(
        reg.capturer(&source).is_none(),
        "the closed source's capturer is gone"
    );
    assert!(
        !reg.capturer_sources().contains(&source),
        "the registry no longer lists it: {:?}",
        reg.capturer_sources()
    );
    assert!(
        state
            .take_screen_control(&source, ControlHolder::User)
            .is_err(),
        "the parked controller went with the source"
    );
    assert!(
        !state.close_remote_screen(&source),
        "closing twice is a no-op"
    );
}

#[test]
fn take_screen_control_without_an_opened_remote_desktop_errors() {
    let (_dir, state) = tmp_state();
    let err = state
        .take_screen_control("remote-xrdp:vm:3389", ControlHolder::User)
        .unwrap_err();
    assert!(
        err.to_string().contains("no controller to take"),
        "the error should say there is no controller, got: {err}"
    );
    // The local source is not a taken remote desktop: its own adapter registers
    // both halves, because the person at the machine is the only writer there.
    let err = state
        .take_screen_control(goble_screen_adapter::LOCAL_SOURCE, ControlHolder::Agent)
        .unwrap_err();
    assert!(
        err.to_string().contains("not opened as a remote desktop"),
        "got: {err}"
    );
}

/// A desktop the host opened the way a handoff does — a capturer in the
/// registry and the controller parked for it — with a stub controller standing
/// in for the RDP client (there is no live xrdp host here, so what these cases
/// prove is the control path, not a click landing on a real desktop).
fn opened_desktop(state: &DesktopState, source: &str) -> Arc<goble_screen_core::MockController> {
    let ctl = Arc::new(goble_screen_core::MockController::new(source));
    state.screen_registry().register_capturer(Arc::new(
        goble_screen_core::MockCapturer::new(source).with_blank(2, 2),
    ));
    state.record_open_desktop(
        source,
        "vm",
        3389,
        Arc::clone(&ctl) as Arc<dyn goble_screen_core::ScreenController>,
        goble_screen_core::ClientLiveness::new(),
    );
    ctl
}

/// C6: the agent's input takes an unheld desktop. A handed-off source is
/// view-only (`open_remote_screen` registers its capturer alone), so the
/// agent's first `click`/`type_text`/`scroll` takes the parked controller
/// instead of failing with `NoController`.
#[test]
fn the_agents_input_takes_an_unheld_desktop() {
    let source = "remote-xrdp:vm:3389";
    let (_dir, state) = tmp_state();
    let ctl = opened_desktop(&state, source);
    let reg = state.screen_registry();

    assert!(!reg.has_control(source), "a fresh handoff is view-only");
    assert!(matches!(
        reg.click(source, 1, 1)
            .unwrap_err()
            .downcast_ref::<goble_screen_core::ScreenError>(),
        Some(goble_screen_core::ScreenError::NoController(s)) if s == source
    ));

    state
        .click_as(ControlHolder::Agent, source, 4, 5)
        .expect("the agent's input takes the desktop it was asked for");
    assert_eq!(reg.control_holder(source), Some(ControlHolder::Agent));
    assert!(reg.controller(source).is_some(), "the take registered it");
    state
        .type_text_as(ControlHolder::Agent, source, "hi")
        .unwrap();
    state
        .scroll_as(ControlHolder::Agent, source, -1, 2)
        .unwrap();

    assert_eq!(
        ctl.actions(),
        vec![
            goble_screen_core::MockAction::Click { x: 4, y: 5 },
            goble_screen_core::MockAction::TypeText { text: "hi".into() },
            goble_screen_core::MockAction::Scroll { dx: -1, dy: 2 },
        ],
        "every input reached the controller the host parked"
    );
    assert!(
        reg.controller_sources().contains(&source.to_string()),
        "the source is driven now: {:?}",
        reg.controller_sources()
    );
}

/// C6: the agent may never take over from the user. Its take is refused with
/// `ControlHeld { holder: User }`, the user's control and controller are left
/// exactly as they were, and the agent's input fails with that clear error
/// rather than silently doing nothing.
#[test]
fn a_refused_agent_take_leaves_the_users_control_intact() {
    let source = "remote-xrdp:vm:3389";
    let (_dir, state) = tmp_state();
    let ctl = opened_desktop(&state, source);
    let reg = state.screen_registry();

    // The user takes it by driving it: the app's own input takes the unheld
    // desktop for `User`.
    state.click(source, 1, 1).expect("the user takes it");
    assert_eq!(reg.control_holder(source), Some(ControlHolder::User));

    let err = state
        .take_screen_control(source, ControlHolder::Agent)
        .unwrap_err();
    assert!(err.to_string().contains("driven by the user"), "{err}");
    assert_eq!(
        reg.control_holder(source),
        Some(ControlHolder::User),
        "the refused take changed nothing"
    );
    let registered = reg
        .controller(source)
        .expect("the user's controller stays put");
    assert!(
        Arc::ptr_eq(
            &registered,
            &(Arc::clone(&ctl) as Arc<dyn goble_screen_core::ScreenController>)
        ),
        "the refused take registered nothing of the agent's"
    );

    let err = state
        .click_as(ControlHolder::Agent, source, 9, 9)
        .unwrap_err();
    assert!(err.to_string().contains("driven by the user"), "{err}");
    assert_eq!(
        ctl.actions(),
        vec![goble_screen_core::MockAction::Click { x: 1, y: 1 }],
        "only the user's click reached the controller"
    );
}

/// C6: the user may take the desktop back from the agent at any time. The
/// agent drives, the user's own input takes it over, and the agent is the side
/// refused from then on.
#[test]
fn the_users_input_takes_the_desktop_back_while_the_agent_drives() {
    let source = "remote-xrdp:vm:3389";
    let (_dir, state) = tmp_state();
    let ctl = opened_desktop(&state, source);
    let reg = state.screen_registry();

    state
        .click_as(ControlHolder::Agent, source, 4, 5)
        .expect("the agent takes the unheld desktop");
    assert_eq!(reg.control_holder(source), Some(ControlHolder::Agent));

    // The user's take is not refused — it replaces the agent's hold.
    state
        .click(source, 7, 8)
        .expect("the user takes the desktop back at any time");
    assert_eq!(reg.control_holder(source), Some(ControlHolder::User));
    assert_eq!(
        ctl.actions(),
        vec![
            goble_screen_core::MockAction::Click { x: 4, y: 5 },
            goble_screen_core::MockAction::Click { x: 7, y: 8 },
        ]
    );

    let err = state
        .click_as(ControlHolder::Agent, source, 1, 1)
        .unwrap_err();
    assert!(
        err.to_string().contains("driven by the user"),
        "the agent's next input is refused: {err}"
    );
    assert_eq!(reg.control_holder(source), Some(ControlHolder::User));
    assert_eq!(ctl.actions().len(), 2, "and it reached nothing");
}

/// A stand-in for the host's `xrdp`: a listener that accepts a client and holds
/// it, counting the connections it took. There is no RDP server here, so the
/// session never paints — what these tests need is a client that is *there*,
/// which is exactly what a handoff has to reuse instead of connecting again.
#[cfg(feature = "remote-screen")]
fn accepting_host() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free local port");
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&accepted);
    std::thread::spawn(move || {
        // The accepted connections are held for the listener's life: dropping
        // one would end the client's session, the opposite of what a test that
        // reuses the desktop needs.
        let mut held = Vec::new();
        for conn in listener.incoming() {
            match conn {
                Ok(stream) => {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    held.push(stream);
                }
                Err(_) => break,
            }
        }
    });
    (port, accepted)
}

#[cfg(feature = "remote-screen")]
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(feature = "remote-screen")]
fn host_config(port: u16) -> goble_harness_types::RemoteScreenConfig {
    goble_harness_types::RemoteScreenConfig {
        host: "127.0.0.1".to_string(),
        port,
        credential: "desktop-account".to_string(),
        width: 1280,
        height: 720,
    }
}

/// One desktop per host: the second handoff for a host finds the desktop the
/// first one opened — the same source id, the same client — instead of
/// connecting again, and the recorded mapping answers both ways.
#[test]
#[cfg(feature = "remote-screen")]
fn a_second_handoff_for_the_same_host_reuses_the_desktop_it_opened() {
    use std::sync::atomic::Ordering;

    let (_dir, state) = tmp_state();
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    let (port, accepted) = accepting_host();
    let config = host_config(port);

    let source = state.open_remote_screen(config.clone()).unwrap();
    let client = state
        .screen_registry()
        .capturer(&source)
        .expect("the handoff registered a capturer");
    wait_until("the first client to connect", || {
        accepted.load(Ordering::SeqCst) >= 1
    });

    // The one recorded mapping answers both directions: the source names its
    // host, and the host names its desktop.
    let recorded = state
        .remote_desktop(&source)
        .expect("the source is recorded");
    assert_eq!(recorded.host, "127.0.0.1");
    assert_eq!(recorded.port, port);
    assert!(recorded.alive, "the client is connected: {recorded:?}");
    let for_host = state
        .remote_desktop_for_host("127.0.0.1")
        .expect("the host's desktop");
    assert_eq!(for_host, recorded);

    let again = state.open_remote_screen(config).unwrap();
    assert_eq!(again, source, "the host's desktop, found again");
    let sources: Vec<String> = state
        .screen_registry()
        .capturer_sources()
        .into_iter()
        .filter(|source| source.starts_with("remote-xrdp:"))
        .collect();
    assert_eq!(
        sources,
        vec![source.clone()],
        "no second source was created"
    );
    assert!(
        Arc::ptr_eq(
            &client,
            &state
                .screen_registry()
                .capturer(&source)
                .expect("the source survived")
        ),
        "the desktop kept its client instead of a second one replacing it"
    );
    // Nothing reconnected: a stray second connection would land here.
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the second handoff connected nothing"
    );
}

/// A resumed session finds the desktop its predecessor opened: the handoff is a
/// recorded turn event, so a session that re-attaches and replays that turn
/// asks for the same host again — and the mapping answers with the desktop that
/// is already open, once, instead of connecting a second time.
#[test]
#[cfg(feature = "remote-screen")]
fn a_resumed_session_finds_the_desktop_its_predecessor_opened() {
    use goble_daemon_protocol::DaemonEvent as DE;
    use std::sync::atomic::Ordering;

    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    let (port, accepted) = accepting_host();
    let config = host_config(port);
    let session = goble_harness_types::SessionId::new("chat-1");

    // The turn that asked for the desktop, and the same session asking again
    // after a re-attach replayed it.
    state.translate_daemon_event(DE::ScreenHandoff {
        session_id: session.clone(),
        config: config.clone(),
    });
    state.translate_daemon_event(DE::ScreenHandoff {
        session_id: session,
        config,
    });

    let handoffs: Vec<String> = bus
        .take_events()
        .into_iter()
        .filter(|(name, _)| name == "screen:handoff")
        .map(|(_, payload)| {
            payload
                .get("source")
                .and_then(|v| v.as_str())
                .expect("the handoff names its source")
                .to_string()
        })
        .collect();
    assert_eq!(handoffs.len(), 2, "both asks reached the app");
    assert_eq!(
        handoffs[0], handoffs[1],
        "the resumed session found the desktop its predecessor opened"
    );
    assert!(
        handoffs[0].starts_with("remote-xrdp:127.0.0.1:"),
        "{handoffs:?}"
    );

    let recorded = state
        .remote_desktop_for_host("127.0.0.1")
        .expect("the host's desktop is recorded");
    assert_eq!(recorded.source, handoffs[0]);
    assert!(recorded.alive);
    assert_eq!(
        state
            .screen_registry()
            .capturer_sources()
            .into_iter()
            .filter(|source| source.starts_with("remote-xrdp:"))
            .count(),
        1,
        "finding the desktop again opened no second source"
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "finding the desktop again connected nothing"
    );
}

/// A handoff a paired worker relays lands in the same open path as a local one:
/// the desktop opens the host's desktop and a second relayed handoff for the
/// same host finds it again, so a run whose session lives on a worker is no
/// different from a local one.
#[test]
#[cfg(feature = "remote-screen")]
fn a_worker_relayed_handoff_opens_the_desktop_and_finds_it_again() {
    use std::sync::atomic::Ordering;

    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    let (port, accepted) = accepting_host();
    let worker = WorkerId::generate();
    let config = serde_json::to_value(host_config(port)).unwrap();

    for _ in 0..2 {
        state.handle_worker_message(
            &worker,
            WorkerMessage::ScreenHandoff {
                trace_id: "chat-1".to_string(),
                config: config.clone(),
            },
        );
    }

    let sources: Vec<String> = bus
        .take_events()
        .into_iter()
        .filter(|(name, _)| name == "screen:handoff")
        .map(|(_, payload)| payload["source"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(sources.len(), 2, "both relayed handoffs reached the app");
    assert_eq!(sources[0], sources[1], "the worker's host kept one desktop");
    assert_eq!(
        state.remote_desktop_for_host("127.0.0.1").unwrap().source,
        sources[0]
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        1,
        "the relayed handoffs connected once"
    );
}

/// A handoff whose client is gone reports it instead of showing a stale frame:
/// the desktop is recorded as gone, its capturer serves no frame, and the
/// handoff that asks for it is told and retires the dead desktop rather than
/// handing back the source that would draw the frame it left behind.
#[test]
#[cfg(feature = "remote-screen")]
fn a_handoff_whose_client_is_gone_reports_it_instead_of_a_stale_frame() {
    let (_dir, state) = tmp_state();
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    state
        .set_credential("desktop-account", "goble:hunter2")
        .unwrap();
    // Nothing listens on port 1: the client's session never opens.
    let config = host_config(1);

    let source = state.open_remote_screen(config.clone()).unwrap();
    wait_until("the client to go", || {
        state
            .remote_desktop(&source)
            .map(|desktop| !desktop.alive)
            .unwrap_or(false)
    });
    let recorded = state
        .remote_desktop(&source)
        .expect("the source is recorded");
    assert!(!recorded.alive, "the dead client is recorded as dead");

    // The capture path reports it rather than serving what the client left.
    let err = state.screen_registry().capture(&source).unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<goble_screen_core::ScreenError>(),
            Some(goble_screen_core::ScreenError::ClientGone(s)) if s == &source
        ),
        "{err}"
    );

    // The handoff is told, and the desktop it cannot reuse goes with the news.
    let err = state.open_remote_screen(config).unwrap_err();
    assert!(
        err.to_string().contains("is gone"),
        "the handoff must report the desktop is gone, got: {err}"
    );
    assert!(
        !err.to_string().contains("hunter2"),
        "the report names the desktop, never the account: {err}"
    );
    assert!(
        state.remote_desktop(&source).is_none(),
        "a gone desktop is not the host's desktop"
    );
    assert!(state.remote_desktop_for_host("127.0.0.1").is_none());
    assert!(
        state.screen_registry().capturer(&source).is_none(),
        "no stale frame is left to serve"
    );
    assert!(
        bus.take_events()
            .iter()
            .any(|(name, payload)| name == "screen:closed"
                && payload.get("source").and_then(|v| v.as_str()) == Some(source.as_str())),
        "the app is told to stop drawing it"
    );
}

#[test]
fn test_state_logs_and_chat() {
    let (_dir, state) = tmp_state();
    state.add_log("hello");
    assert_eq!(state.get_logs().len(), 1);
    let chat_id = state.create_chat("Test chat", None, None).unwrap();
    state
        .add_chat_message(&chat_id, "user", "hello agent")
        .unwrap();
    let msgs = state.list_chat_messages(&chat_id).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "hello agent");
}

#[test]
fn test_state_agent_and_workflow() {
    let (_dir, state) = tmp_state();
    let agent = state
        .create_agent("greeter", "say hello", Some("test agent"), vec![])
        .unwrap();
    assert_eq!(state.list_agents().len(), 1);
    let step = WorkflowStep {
        id: uuid::Uuid::new_v4().to_string(),
        name: "greet".to_string(),
        agent_id: AgentId(agent.id.clone()),
        input_template: "Greet the user".to_string(),
        depends_on: vec![],
    };
    let wf = state
        .create_workflow("hello", "Hello workflow", vec![step], Trigger::Manual)
        .unwrap();
    assert_eq!(state.list_workflows().len(), 1);
    assert_eq!(wf.steps.len(), 1);
    state.delete_workflow(&WorkflowId(wf.id)).unwrap();
    assert!(state.list_workflows().is_empty());
    state.delete_agent(&AgentId(agent.id)).unwrap();
    assert!(state.list_agents().is_empty());
}

#[test]
fn test_state_team_and_vault() {
    let (_dir, state) = tmp_state();
    state.set_vault_passphrase("passphrase".to_string());
    state.set_vault_secret("api_key", "sk-123").unwrap();
    assert_eq!(state.list_vault_secrets().len(), 1);
    state
        .create_team("team1", "Platform", "{}", vec!["a1".to_string()])
        .unwrap();
    assert_eq!(state.list_teams().len(), 1);
}

#[test]
fn test_worker_message_handling() {
    let (_dir, state) = tmp_state();
    let wid = WorkerId::generate();
    state
        .add_worker(
            wid.clone(),
            "vps".to_string(),
            "ws://localhost:8787/ws".to_string(),
        )
        .unwrap();
    state.handle_worker_message(&wid, WorkerMessage::Paired);
    assert!(state.list_workers()[0].paired);
    state.handle_worker_message(
        &wid,
        WorkerMessage::AgentLog {
            trace_id: "t1".to_string(),
            step_id: "s1".to_string(),
            level: goble_core::execution::LogLevel::Info,
            message: "hello".to_string(),
        },
    );
    assert_eq!(state.get_logs().len(), 2);
}

#[test]
fn test_agent_workflow_team_vault_roundtrip() {
    let (_dir, state) = tmp_state();
    state.set_vault_passphrase("secret".to_string());
    let agent = state
        .create_agent("greeter", "say hello", Some("test agent"), vec![])
        .unwrap();
    let step = WorkflowStep {
        id: uuid::Uuid::new_v4().to_string(),
        name: "greet".to_string(),
        agent_id: AgentId(agent.id.clone()),
        input_template: "Greet the user".to_string(),
        depends_on: vec![],
    };
    let wf = state
        .create_workflow(
            "hello",
            "Hello workflow",
            vec![step],
            goble_core::agent::Trigger::Manual,
        )
        .unwrap();
    assert_eq!(state.list_agents().len(), 1);
    assert_eq!(state.list_workflows().len(), 1);

    state
        .create_team("team1", "Platform", "{}", vec![agent.id.clone()])
        .unwrap();
    assert_eq!(state.list_teams().len(), 1);
    assert_eq!(state.list_teams()[0].members.len(), 1);

    state.set_vault_secret("api_key", "sk-123").unwrap();
    assert_eq!(state.list_vault_secrets().len(), 1);

    state.delete_workflow(&WorkflowId(wf.id)).unwrap();
    state.delete_agent(&AgentId(agent.id)).unwrap();
    assert!(state.list_workflows().is_empty());
    assert!(state.list_agents().is_empty());
}

#[test]
fn test_persistence_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("store.db")).unwrap();
    let state = DesktopState::new(
        store,
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    state.set_vault_passphrase("p".to_string());
    let agent = state.create_agent("a", "prompt", None, vec![]).unwrap();
    let step = WorkflowStep {
        id: uuid::Uuid::new_v4().to_string(),
        name: "s".to_string(),
        agent_id: AgentId(agent.id.clone()),
        input_template: "in".to_string(),
        depends_on: vec![],
    };
    state
        .create_workflow("wf", "desc", vec![step], goble_core::agent::Trigger::Manual)
        .unwrap();
    state
        .create_team("t", "Team", "{}", vec![agent.id])
        .unwrap();
    state.set_vault_secret("k", "v").unwrap();

    let state2 = DesktopState::new(
        Store::open(tmp.path().join("store.db")).unwrap(),
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    state2.load_from_store().unwrap();
    assert_eq!(state2.list_agents().len(), 1);
    assert_eq!(state2.list_workflows().len(), 1);
    assert_eq!(state2.list_teams().len(), 1);
    assert_eq!(state2.list_vault_secrets().len(), 1);
}

#[test]
fn test_cluster_identity_encrypted_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("store.db")).unwrap();
    let state = DesktopState::new(
        store,
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    let identity = state.create_cluster("test-cluster", "secret-pass").unwrap();
    assert!(!identity.cluster_name.is_empty());

    let state2 = DesktopState::new(
        Store::open(tmp.path().join("store.db")).unwrap(),
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    assert!(state2.has_stored_cluster_identity());
    assert!(state2.unlock_cluster_identity("wrong").is_err());
    assert!(state2.unlock_cluster_identity("secret-pass").unwrap());
    let loaded = state2.get_cluster_identity().unwrap();
    assert_eq!(loaded.cluster_name, identity.cluster_name);
}

#[test]
fn migrate_legacy_chats_creates_threads() {
    let (_dir, state) = tmp_state();
    // Create a legacy chat with two messages
    state.create_chat("legacy chat", None, None).unwrap();
    let chats = state.list_chats();
    let chat = &chats[0];
    state.add_chat_message(&chat.id, "user", "hello").unwrap();
    state
        .add_chat_message(&chat.id, "user", "hi there")
        .unwrap();

    let threads = state.migrate_legacy_chats_to_threads().unwrap();
    assert_eq!(threads.len(), 1);
    let summary = &threads[0];
    assert_eq!(summary.title, "legacy chat");

    let messages = state
        .thread_store()
        .list_messages(&goble_core::thread::ThreadId(summary.id.clone()))
        .unwrap();
    assert_eq!(messages.len(), 2);
}

/// A conversation's title is written to the store, into the in-memory list the
/// sidebar is built from, and onto the bus as `chats:updated` — the write path
/// a subject derived from the conversation's own content takes, so the app
/// re-reads the list and shows the new subject.
#[test]
fn setting_a_chat_title_reaches_the_store_the_list_and_the_bus() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let state = DesktopState::new(
        store.clone(),
        crate::thread_store::ThreadStore::new(dir.path()).unwrap(),
    );
    let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
    state.set_event_bus(bus.clone());
    let chat_id = state
        .create_chat("New conversation", None, None)
        .expect("create chat");
    // The creation's own event is taken away, so the one below is this write's.
    bus.take_events();

    state
        .set_chat_title(&chat_id, "Initial user onboarding")
        .expect("retitle the conversation");

    assert_eq!(
        store.get_chat_title(&chat_id).unwrap(),
        Some("Initial user onboarding".to_string()),
        "the title is durable"
    );
    assert_eq!(
        state.list_chats()[0].title,
        "Initial user onboarding",
        "the list the sidebar is built from carries it"
    );
    let events = bus.take_events();
    assert!(
        events.iter().any(|(name, _)| name == "chats:updated"),
        "the write is announced so the app re-reads the list: {events:?}"
    );

    // Retitling one conversation leaves its neighbours' titles alone.
    let other = state.create_chat("Kept", None, None).expect("create chat");
    state
        .set_chat_title(&chat_id, "Sidebar subjects")
        .expect("retitle again");
    let listed = state.list_chats();
    let kept = listed
        .iter()
        .find(|c| c.id == other)
        .expect("the other chat");
    assert_eq!(kept.title, "Kept");
    assert_eq!(
        store.get_chat_title(&chat_id).unwrap(),
        Some("Sidebar subjects".to_string())
    );
}
