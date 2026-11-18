//! One harness session per conversation: which session a card claims in a
//! directory that holds several, and the name that session carries.

use super::*;

use std::path::Path;

/// `GROK_HOME` and `ANTIGRAVITY_HOME` are process-wide, so a test that points
/// one at a temp store must not overlap another that reads a store.
static SESSION_STORE_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Restores a harness's home even when an assertion fails mid-test.
struct HomeGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn home_at(key: &'static str, dir: &Path) -> HomeGuard {
    let previous = std::env::var_os(key);
    std::env::set_var(key, dir);
    HomeGuard { key, previous }
}

/// The directory Grok names a working directory with, percent-encoded with the
/// URL unreserved set kept.
fn encoded_cwd(cwd: &str) -> String {
    let mut encoded = String::new();
    for byte in cwd.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// A Grok session directory with the summary Grok writes for it.
fn grok_session(root: &Path, cwd: &str, id: &str, title: &str, updated_at: &str) {
    let dir = root.join("sessions").join(encoded_cwd(cwd)).join(id);
    std::fs::create_dir_all(&dir).expect("create the session directory");
    std::fs::write(
        dir.join("summary.json"),
        format!(r#"{{"session_summary":"{title}","updated_at":"{updated_at}"}}"#),
    )
    .expect("write the session summary");
}

fn a_desktop(dir: &Path) -> std::sync::Arc<DesktopState> {
    DesktopState::new(
        goble_core::store::Store::open_in_memory().expect("in-memory store"),
        goble_desktop_service::ThreadStore::new(dir).expect("thread store"),
    )
}

fn card(desktop: &DesktopState, id: &str) -> goble_desktop_service::Chat {
    desktop
        .list_chats()
        .into_iter()
        .find(|chat| chat.id == id)
        .expect("the conversation is a card")
}

fn card_title(state: &UiState, id: &str) -> String {
    state
        .conversations
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.name.clone())
        .expect("the card is in the sidebar list")
}

fn sqlite3_available() -> bool {
    std::process::Command::new("sqlite3")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn two_grok_conversations_in_one_directory_keep_their_own_sessions_and_titles() {
    let _env = SESSION_STORE_ENV
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let home = tempfile::tempdir().expect("temp grok home");
    let _grok = home_at("GROK_HOME", home.path());
    let cwd = "/Users/me/Projects/goble";
    grok_session(
        home.path(),
        cwd,
        "session-a",
        "First conversation",
        "2026-11-10T22:07:00.000000Z",
    );
    grok_session(
        home.path(),
        cwd,
        "session-b",
        "Second conversation",
        "2026-11-10T23:06:00.000000Z",
    );

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = a_desktop(dir.path());
    let store = desktop.store_clone();
    for (id, when) in [
        ("chat-a", "2026-11-10T22:07:00Z"),
        ("chat-b", "2026-11-10T23:06:00Z"),
        ("chat-c", "2026-11-10T23:20:00Z"),
    ] {
        store
            .insert_chat(id, "Grok", Some("grok"), None, when, when)
            .expect("insert the conversation");
    }
    desktop.load_from_store().expect("load the store");
    for id in ["chat-a", "chat-b", "chat-c"] {
        desktop
            .set_chat_working_dir(id, cwd)
            .expect("record the directory");
    }

    let mut state = UiState::mock();
    state.refresh_conversations(&desktop);

    assert_eq!(
        card(&desktop, "chat-a").agent_id.as_deref(),
        Some("session-a"),
        "the card keeps the session whose time matches its own"
    );
    assert_eq!(
        card(&desktop, "chat-b").agent_id.as_deref(),
        Some("session-b"),
        "the second card claims the session the first did not"
    );
    assert_eq!(
        card(&desktop, "chat-c").agent_id,
        None,
        "a conversation with no session of its own claims none"
    );
    assert_eq!(card_title(&state, "chat-a"), "First conversation");
    assert_eq!(card_title(&state, "chat-b"), "Second conversation");
    assert_eq!(
        card_title(&state, "chat-c"),
        "Grok",
        "a card with no session keeps the harness's name"
    );
}

#[test]
fn a_conversation_keeps_the_session_it_claimed_when_a_newer_one_appears() {
    let _env = SESSION_STORE_ENV
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let home = tempfile::tempdir().expect("temp grok home");
    let _grok = home_at("GROK_HOME", home.path());
    let cwd = "/Users/me/Projects/goble";
    grok_session(
        home.path(),
        cwd,
        "session-a",
        "First conversation",
        "2026-11-10T22:07:00.000000Z",
    );

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = a_desktop(dir.path());
    let store = desktop.store_clone();
    store
        .insert_chat(
            "chat-a",
            "Grok",
            Some("grok"),
            None,
            "2026-11-10T22:07:00Z",
            "2026-11-10T22:07:00Z",
        )
        .expect("insert the conversation");
    desktop.load_from_store().expect("load the store");
    desktop
        .set_chat_working_dir("chat-a", cwd)
        .expect("record the directory");

    let mut state = UiState::mock();
    state.refresh_conversations(&desktop);
    assert_eq!(card_title(&state, "chat-a"), "First conversation");

    // A newer session lands in the same directory, as a second conversation's
    // harness run would leave one.
    grok_session(
        home.path(),
        cwd,
        "session-b",
        "Second conversation",
        "2026-11-10T23:06:00.000000Z",
    );
    let spec = goble_core::harness::harness_by_id("grok").expect("grok is in the catalog");
    let claimed = state.harness_session_id("chat-a", spec, cwd, Some(&desktop));
    assert_eq!(
        claimed.as_deref(),
        Some("session-a"),
        "the card keeps the session it already recorded"
    );
    assert_eq!(
        crate::terminal::session_title("grok", cwd).as_deref(),
        Some("Second conversation"),
        "the newest session of the directory is another conversation's"
    );
    assert_eq!(
        crate::terminal::session_title_for("grok", cwd, "session-a").as_deref(),
        Some("First conversation"),
        "the card names itself from its own session"
    );

    state.refresh_conversations(&desktop);
    assert_eq!(
        card_title(&state, "chat-a"),
        "First conversation",
        "a newer session does not rename the card"
    );
}

#[test]
fn an_antigravity_conversation_is_named_by_the_row_its_conversation_owns() {
    if !sqlite3_available() {
        eprintln!("sqlite3 is not installed; skipping the antigravity mapping case");
        return;
    }
    let _env = SESSION_STORE_ENV
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let home = tempfile::tempdir().expect("temp antigravity home");
    let _antigravity = home_at("ANTIGRAVITY_HOME", home.path());
    let cwd = "/Users/me/Projects/goble";
    std::fs::create_dir_all(home.path().join("cache")).expect("create the cache directory");
    std::fs::write(
        home.path().join("cache").join("last_conversations.json"),
        format!(r#"{{"{cwd}": "conv-b"}}"#),
    )
    .expect("write the cache");
    let status = std::process::Command::new("sqlite3")
        .arg(home.path().join("conversation_summaries.db"))
        .arg(
            "create table conversation_summaries (conversation_id text primary key, \
             title text, preview text, workspace_uris text, last_modified_time text);\
             insert into conversation_summaries values ('conv-a', 'First antigravity', '', \
             '[\"file:///Users/me/Projects/goble\"]', '2026-11-10 22:07:00.000000+00:00');\
             insert into conversation_summaries values ('conv-b', 'Second antigravity', '', \
             '[\"file:///Users/me/Projects/goble\"]', '2026-11-10 23:06:00.000000+00:00');",
        )
        .status()
        .expect("run sqlite3");
    assert!(status.success(), "sqlite3 built the summaries store");

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = a_desktop(dir.path());
    let store = desktop.store_clone();
    for (id, when) in [
        ("chat-a", "2026-11-10T22:07:00Z"),
        ("chat-b", "2026-11-10T23:06:00Z"),
    ] {
        store
            .insert_chat(id, "Antigravity", Some("antigravity"), None, when, when)
            .expect("insert the conversation");
    }
    desktop.load_from_store().expect("load the store");
    for id in ["chat-a", "chat-b"] {
        desktop
            .set_chat_working_dir(id, cwd)
            .expect("record the directory");
    }

    let mut state = UiState::mock();
    state.refresh_conversations(&desktop);

    assert_eq!(card(&desktop, "chat-a").agent_id.as_deref(), Some("conv-a"));
    assert_eq!(card(&desktop, "chat-b").agent_id.as_deref(), Some("conv-b"));
    assert_eq!(card_title(&state, "chat-a"), "First antigravity");
    assert_eq!(card_title(&state, "chat-b"), "Second antigravity");
}

#[test]
fn a_session_id_round_trips_through_the_store_and_the_cache() {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = a_desktop(dir.path());
    let chat = desktop
        .create_chat("Grok", Some("grok"), None)
        .expect("create the conversation");
    assert_eq!(card(&desktop, &chat).agent_id, None);

    desktop
        .set_chat_agent_id(&chat, Some("session-a"))
        .expect("record the session");
    assert_eq!(
        card(&desktop, &chat).agent_id.as_deref(),
        Some("session-a"),
        "the cache the sidebar reads carries the session"
    );
    assert_eq!(
        desktop
            .store_clone()
            .get_chat_agent_id(&chat)
            .expect("read the store"),
        Some("session-a".to_string()),
        "the row is durable"
    );

    let reloaded = DesktopState::new(
        desktop.store_clone(),
        goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
    );
    reloaded.load_from_store().expect("load the store");
    assert_eq!(
        card(&reloaded, &chat).agent_id.as_deref(),
        Some("session-a"),
        "a restart reads the session back"
    );

    desktop
        .set_chat_agent_id(&chat, None)
        .expect("clear the session");
    assert_eq!(card(&desktop, &chat).agent_id, None);
    assert_eq!(
        desktop
            .store_clone()
            .get_chat_agent_id(&chat)
            .expect("read the store"),
        None,
        "clearing reaches the row too"
    );
}
