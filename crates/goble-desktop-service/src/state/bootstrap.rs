use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use goble_core::mcp_manager::McpManager;
use goble_core::store::Store;
use goble_core::vault::CredentialVault;
use goble_harness_types::SessionId;
use parking_lot::Mutex;
use serde::Serialize;

use crate::event_bus::{emit_value, EventBus, NoOpEventBus};
use crate::thread_store::ThreadStore;

use super::DesktopState;

/// One remote desktop this state has open, under the source id the registry
/// holds it by: the host it reaches, the port it was opened on, the controller
/// nobody holds yet, and the client's own liveness.
///
/// This is the source-id → host mapping in one place — the answer to "which
/// desktop is this host's" is read from it and nowhere else — so a second
/// handoff for a host finds the desktop that is already open instead of
/// connecting again, and a desktop whose client has gone is recorded as gone
/// rather than read as a live one.
pub(crate) struct RemoteDesktop {
    source: String,
    host: String,
    port: u16,
    /// The controller the source registered its capturer without: a remote
    /// desktop is view-only until somebody takes it.
    controller: Arc<dyn goble_screen_core::ScreenController>,
    /// The client's liveness — the same handle its capturer consults before it
    /// serves a frame, so "the client is gone" is one recorded fact and not two
    /// readings of it.
    liveness: goble_screen_core::ClientLiveness,
}

impl RemoteDesktop {
    fn info(&self) -> RemoteDesktopInfo {
        RemoteDesktopInfo {
            source: self.source.clone(),
            host: self.host.clone(),
            port: self.port,
            alive: self.liveness.is_alive(),
        }
    }
}

/// What the recorded mapping answers for one open remote desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDesktopInfo {
    /// The source id the registry holds the desktop under.
    pub source: String,
    /// The host it reaches.
    pub host: String,
    /// The RDP port it was opened on.
    pub port: u16,
    /// Whether the desktop's client is still there. A desktop whose client is
    /// gone serves no frame, so a caller reports it rather than drawing it.
    pub alive: bool,
}

/// Copy legacy state into the new `~/.goble` home the first time it appears, so
/// an existing installation is not left behind. Legacy store was a bare relative
/// path (`goble_store.sqlite` in the CWD) and threads lived in
/// `dirs::data_dir()/com.goble.desktop/threads`. A migration only runs when the
/// new home file/dir is absent, so it never clobbers a fresh home.
fn migrate_legacy_home(home: &goble_core::app_home::GobleHome) {
    let legacy_store = Path::new("goble_store.sqlite");
    if legacy_store.exists() && !home.store_path().exists() {
        if let Err(e) = fs::copy(legacy_store, home.store_path()) {
            log::warn!("migrate store failed: {e}");
        } else {
            record_migration(home, "store", &legacy_store.to_string_lossy());
        }
    }

    let legacy_threads = dirs::data_dir()
        .map(|d| d.join("com.goble.desktop").join("threads"));
    if let Some(src) = legacy_threads {
        if src.is_dir() && !home.threads_dir().exists() {
            if let Err(e) = copy_dir_all(&src, &home.threads_dir()) {
                log::warn!("migrate threads failed: {e}");
            } else {
                record_migration(home, "threads", &src.to_string_lossy());
            }
        }
    }
}

fn record_migration(home: &goble_core::app_home::GobleHome, kind: &str, source: &str) {
    let note = home.root().join("last-copy.txt");
    let mut text = std::fs::read_to_string(&note).unwrap_or_default();
    text.push_str(&format!("{kind}: {source}\n"));
    let _ = std::fs::write(&note, text);
}

fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

impl DesktopState {
    pub fn open_default() -> anyhow::Result<Arc<Self>> {
        let home = goble_core::app_home::GobleHome::locate()?;
        // Every user gets the base home (identity/auth/config/sessions/logs).
        // The workspace payload (bundled tooling, worktrees, threads) and a local
        // store are only materialized when the workspace runs on this machine.
        // Routing is not wired yet, so today every deployment is local; when it
        // lands, a remote-only workspace skips `ensure_workspace()` and the local
        // store and runs as a thin client against the remote worker.
        home.ensure_base()?;
        home.ensure_workspace()?;
        migrate_legacy_home(&home);
        let store = Store::open(home.store_path())?;
        let thread_store = ThreadStore::new(home.threads_dir())?;
        let state = Self::new(store, thread_store);
        state.set_environment_path(home.environment_path());
        state.set_config_path(home.config_path());
        let _ = state.reload_config(&home.config_path());
        let _ = state.load_from_store();
        // Make the reversibility ledger durable: persist settled checkpoints to
        // SQLite and seed the daemon's ledgers from anything persisted before, so
        // rewind/fork/replay survive a restart. The daemon keeps the store alive
        // via the checkpoint sink.
        let checkpoint_path = home.root().join("checkpoints.sqlite");
        match goble_persistence::CheckpointStore::open(&checkpoint_path) {
            Ok(store) => {
                let store = Arc::new(store);
                for session in store.list_checkpoint_sessions().unwrap_or_default() {
                    if let Some(cp) = store.load_checkpoint(&session).unwrap_or(None) {
                        state.daemon_state.restore_checkpoint(&cp);
                    }
                }
                state.daemon_state.set_checkpoint_sink(Some(
                    Arc::clone(&store) as Arc<dyn goble_replay::CheckpointSink>,
                ));
                // Seed the durable Project/Session/Task entities alongside the
                // checkpoints so a restart rehydrates the workspace metadata.
                for p in store.list_projects().unwrap_or_default() {
                    state.projects.lock().insert(p.project_id.clone(), p);
                }
                for s in store.list_sessions().unwrap_or_default() {
                    state.sessions.lock().insert(s.session_id.clone(), s);
                }
                for t in store.list_tasks().unwrap_or_default() {
                    state.tasks.lock().insert(t.task_id.clone(), t);
                }
            }
            Err(e) => log::warn!("replay checkpoint store unavailable: {e}"),
        }
        Ok(state)
    }

    /// Load `~/.goble/config.toml` into memory. A missing file leaves the
    /// in-memory config as it is; a file that is not TOML at all is logged and
    /// reported, and the running config is kept — a bad edit must not wipe the
    /// models the app is configured with. A section that does not parse costs
    /// only itself; the sections around it are kept.
    pub fn reload_config(&self, path: &Path) -> anyhow::Result<()> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                log::error!("cannot read {}: {e}", path.display());
                return Err(anyhow::Error::new(e).context(format!("read {}", path.display())));
            }
        };
        match goble_core::config::GobleConfig::load_toml(&text) {
            Ok((config, problems)) => {
                for problem in &problems {
                    log::error!("{}: {problem}", path.display());
                }
                *self.config.lock() = config;
                Ok(())
            }
            Err(e) => {
                log::error!(
                    "{} is not valid TOML, keeping the running config: {e}",
                    path.display()
                );
                Err(e)
            }
        }
    }

    /// The agent-visible configuration, resolved from `~/.goble/config.toml`.
    pub fn config(&self) -> goble_core::config::GobleConfig {
        self.config.lock().clone()
    }

    /// Persist the config to this state's `config.toml` and update the in-memory
    /// copy. A legacy `[llm]` section is folded into `[model.*]` / `[models]`
    /// here, so every save writes the adopted schema.
    ///
    /// A state built with [`DesktopState::new`] has no home, so this refuses
    /// rather than guessing one: a save is what a running app does, and a state
    /// whose home nobody named is a test or a tool that must say where it writes.
    pub fn save_config(&self, config: &goble_core::config::GobleConfig) -> anyhow::Result<()> {
        let Some(path) = self.config_path() else {
            anyhow::bail!(
                "this state has no config path: pin one with `set_config_path` (the running app gets it from `open_default`)"
            );
        };
        let mut config = config.clone();
        config.absorb_legacy();
        let toml = config.to_toml()?;
        fs::write(&path, toml).with_context(|| format!("write {}", path.display()))?;
        *self.config.lock() = config;
        Ok(())
    }

    /// The `config.toml` this state saves to, or `None` when it was built
    /// without a home.
    pub fn config_path(&self) -> Option<PathBuf> {
        self.config_path.lock().clone()
    }

    /// Point the config accessors at `path` instead of `~/.goble/config.toml`.
    pub fn set_config_path(&self, path: impl Into<PathBuf>) {
        *self.config_path.lock() = Some(path.into());
    }

    pub fn new(store: Store, thread_store: ThreadStore) -> Arc<Self> {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        let sink: Arc<dyn goble_daemon::DaemonEventSink> =
            Arc::new(goble_daemon_client::BroadcastSink::new(tx.clone()));
        let daemon_state = goble_daemon::DaemonState::new(
            goble_harness_runtime::HarnessRegistry::new(),
            sink,
        );
        let daemon: Arc<dyn goble_daemon_client::DaemonClient> =
            Arc::new(goble_daemon_client::InProcessClient::new(daemon_state.clone(), tx));
        // Seed the screen registry with the local adapter. On macOS this is the
        // real `screencapture`-backed capturer; elsewhere the adapter supplies a
        // synthetic pair, so a capturer/controller is always reachable. Only the
        // real grab runs at capture time, so registering never needs permission.
        let screen = goble_screen_core::ScreenRegistry::new();
        if let Err(e) = goble_screen_adapter::LocalAdapter::connect_and_register(
            goble_screen_adapter::LOCAL_SOURCE,
            &screen,
        ) {
            log::warn!("screen adapter unavailable: {e}");
        }
        Arc::new(Self {
            store: Arc::new(Mutex::new(store)),
            workers: Arc::new(Mutex::new(HashMap::new())),
            chats: Arc::new(Mutex::new(Vec::new())),
            messages: Arc::new(Mutex::new(HashMap::new())),
            agents: Arc::new(Mutex::new(HashMap::new())),
            workflows: Arc::new(Mutex::new(HashMap::new())),
            teams: Arc::new(Mutex::new(HashMap::new())),
            projects: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            executions: Arc::new(Mutex::new(HashMap::new())),
            vault: Arc::new(Mutex::new(CredentialVault::new())),
            vault_passphrase: Mutex::new(Vec::new()),
            logs: Arc::new(Mutex::new(Vec::new())),
            clients: Arc::new(Mutex::new(HashMap::new())),
            mcp_manager: McpManager::new(),
            event_bus: Mutex::new(Arc::new(NoOpEventBus)),
            cluster_identity: Mutex::new(None),
            thread_store: Arc::new(thread_store),
            config: parking_lot::Mutex::new(goble_core::config::GobleConfig::default()),
            config_path: Mutex::new(None),
            environment: Mutex::new(super::environment::EnvironmentCache::default()),
            daemon_state,
            daemon,
            translator_spawned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            tool_in_flight: Arc::new(Mutex::new(HashMap::new())),
            screen,
            remote_desktops: Mutex::new(HashMap::new()),
        })
    }

    pub fn thread_store(&self) -> Arc<ThreadStore> {
        Arc::clone(&self.thread_store)
    }

    /// The screen capture+control registry, seeded with the local adapter.
    pub fn screen_registry(&self) -> &goble_screen_core::ScreenRegistry {
        &self.screen
    }

    /// The open remote desktop `source` names, as the recorded source-id → host
    /// mapping answers it. `None` when nothing is open under that source —
    /// never opened, or closed.
    pub fn remote_desktop(&self, source: &str) -> Option<RemoteDesktopInfo> {
        self.remote_desktops
            .lock()
            .get(source)
            .map(RemoteDesktop::info)
    }

    /// Which desktop is `host`'s: the open desktop recorded for it, read from
    /// the one mapping. `None` when no desktop is open for the host.
    ///
    /// The desktop belongs to the host, so this is the identity the handoff
    /// consults: a host that already has one is found again, whatever the
    /// handoff turns out to name. `alive` says whether that desktop still has a
    /// client — a desktop that is recorded but gone must be reported, not drawn.
    pub fn remote_desktop_for_host(&self, host: &str) -> Option<RemoteDesktopInfo> {
        self.remote_desktops
            .lock()
            .values()
            .find(|desktop| desktop.host == host)
            .map(RemoteDesktop::info)
    }

    /// Open a live remote desktop (xrdp / RDP) for `config` and register it into
    /// the screen registry under a derived source id. This is the host side of a
    /// harness [`ScreenHandoff`](goble_daemon_protocol::DaemonEvent::ScreenHandoff):
    /// the GUI then lists the new source and routes broadcast/computer-use input
    /// to it through the registry, identically to the local adapter.
    ///
    /// **One desktop per host.** A host that already has a desktop open is
    /// answered with it — the same source id, no second connection — because the
    /// desktop belongs to the host and is recorded in one place
    /// ([`Self::remote_desktop_for_host`]). The host is the identity, so a
    /// handoff naming a different port for a host that already has one is
    /// answered with the desktop that is open, and the record carries the port
    /// it was opened on. A desktop recorded as gone is not answered at all: it
    /// is retired (the source leaves the registry, so no stale frame is served)
    /// and reported, and the next handoff for that host opens it afresh.
    ///
    /// The desktop is reached with the credential `config` *names*, resolved
    /// here — the account is never carried in the handoff. A name that is not
    /// stored, or whose value is not an account line, fails loudly: a desktop
    /// opened without an account would have to log in by hand, which is not
    /// something the agent asked for. It is read only where a connection is
    /// actually built, so a handoff that finds the host's desktop already open
    /// is answered without re-reading the credential.
    ///
    /// The opened desktop is **view-only**: the remote source registers its
    /// capturer alone, because the agent that asked for it and the user watching
    /// it can both reach it and only one may drive. Its controller is kept here
    /// for [`Self::take_screen_control`].
    ///
    /// Returns the registered source id. Without the `remote-screen` feature this
    /// fails with a clear message instead of silently doing nothing.
    pub fn open_remote_screen(
        &self,
        config: goble_harness_types::RemoteScreenConfig,
    ) -> anyhow::Result<String> {
        #[cfg(feature = "remote-screen")]
        {
            let goble_harness_types::RemoteScreenConfig {
                host,
                port,
                credential,
                width,
                height,
            } = config;
            // The mapping is consulted before anything is connected, so a second
            // handoff for this host — a resumed session replaying the turn that
            // asked for it, say — finds the desktop its predecessor opened.
            if let Some(open) = self.remote_desktop_for_host(&host) {
                if open.alive {
                    return Ok(open.source);
                }
                // The client is gone: the desktop would answer with the last
                // frame it left behind, so it is retired instead and the handoff
                // is told. A later handoff for the host opens it again.
                self.close_remote_screen(&open.source);
                anyhow::bail!(
                    "the remote desktop for host '{host}' is gone: its client ended, so {} serves \
                     no frame and this handoff opens no second desktop for the host",
                    open.source
                );
            }
            let (username, password) = self.resolve_screen_credential(&credential)?;
            let source = format!("remote-xrdp:{host}:{port}");
            let remote = goble_screen_sdk::RemoteConfig {
                host: host.clone(),
                port,
                username,
                password,
                width,
                height,
            };
            let connected =
                goble_screen_sdk::RdpRemoteSource::connect(&source, remote, &self.screen)?;
            // The registry holds the capturer (which keeps the session alive);
            // the controller is the caller's, parked here until a holder takes
            // the desktop. The client's liveness is parked with it, so the
            // mapping can say whether the desktop still has a client.
            self.record_open_desktop(
                source.clone(),
                host,
                port,
                Arc::clone(&connected.controller),
                connected.liveness,
            );
            Ok(source)
        }
        #[cfg(not(feature = "remote-screen"))]
        {
            let _ = config;
            anyhow::bail!(
                "remote screen support is not enabled; build goble-desktop-service with the \
                 `remote-screen` cargo feature"
            )
        }
    }

    /// Resolve the desktop account a handoff names into `(username, password)`,
    /// where the RDP connection is built. The credential is read through the
    /// same store the local credential path uses
    /// ([`Self::get_credential`]), and its value is the **account line**
    /// `username:password` — the reference travels, the secret does not.
    ///
    /// Every failure is loud and names the credential, never the value it holds:
    /// a name that is not stored, an account line without its two halves, and a
    /// handoff that named no credential at all.
    ///
    /// Only the RDP path resolves a credential: without the `remote-screen`
    /// feature `open_remote_screen` reports the missing feature and opens
    /// nothing, so the callers left are the tests that pin the rule.
    #[cfg(any(feature = "remote-screen", test))]
    pub(crate) fn resolve_screen_credential(
        &self,
        credential: &str,
    ) -> anyhow::Result<(String, String)> {
        if credential.trim().is_empty() {
            anyhow::bail!(
                "the screen handoff named no credential; open_screen takes the name of a stored \
                 desktop credential"
            );
        }
        let account = self.get_credential(credential)?.ok_or_else(|| {
            anyhow::anyhow!(
                "no stored credential named `{credential}` for the remote desktop; store the \
                 desktop account first (its value is the account line `username:password`) — the \
                 `credentials` tool lists the stored names"
            )
        })?;
        let (username, password) = account.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "credential `{credential}` does not hold a desktop account; its value must be the \
                 account line `username:password`"
            )
        })?;
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            anyhow::bail!(
                "credential `{credential}` holds an incomplete desktop account: the account line \
                 `username:password` needs both halves"
            );
        }
        Ok((username.to_string(), password.to_string()))
    }

    /// Close a remote desktop [`Self::open_remote_screen`] opened: the
    /// controller parked for it is dropped and the source is unregistered, so
    /// its capturer — and with it the RDP client and the frame buffer it keeps
    /// — goes with the entry and the registry no longer lists the source.
    ///
    /// Dropping the controller is also what ends the remote session itself: the
    /// RDP controller's own drop sends the engine's in-band close, so the client
    /// tears the connection down rather than leaving the host's session running
    /// behind a card nobody watches.
    ///
    /// Closing is not a control release: the registry entry is what a take
    /// registered into, so whoever was driving holds nothing afterwards. The
    /// close is announced as `screen:closed`, which is how the app stops
    /// drawing the source (it drops the frames it held and re-reads the list).
    ///
    /// Returns whether the source was registered; closing one twice, or one
    /// that was never opened, is a no-op.
    pub fn close_remote_screen(&self, source: &str) -> bool {
        // The record goes with the source: a closed desktop is not the host's
        // desktop any more, so a later handoff opens it afresh rather than
        // finding a source the registry no longer holds.
        let recorded = self.remote_desktops.lock().remove(source).is_some();
        let unregistered = self.screen.unregister_source(source);
        if !recorded && !unregistered {
            return false;
        }
        self.emit("screen:closed", serde_json::json!({ "source": source }));
        true
    }

    /// Take a source's input for `holder`, registering the controller
    /// [`Self::open_remote_screen`] kept for it.
    ///
    /// One writer at a time, with the recorded asymmetry the registry enforces:
    /// **the user may take the desktop back from the agent at any time, and the
    /// agent may never take it from the user.** A user take replaces an agent
    /// holder; an agent take while the user drives is refused
    /// ([`goble_screen_core::ScreenError::ControlHeld`]) — and that refusal is
    /// how the agent asks for a desktop the user is driving: retry it once the
    /// user releases and the take succeeds (rakazo refuses its takeover with
    /// HTTP 409 for the same reason and records the request as `waiting_takeover`
    /// on the run). A source opened as a remote desktop is view-only from the
    /// moment it opens until a holder takes it — this call, or an input entry
    /// point ([`Self::click_as`] and its siblings), which takes it first.
    pub fn take_screen_control(
        &self,
        source: &str,
        holder: goble_screen_core::ControlHolder,
    ) -> anyhow::Result<()> {
        let controller = self.remote_controller(source)?;
        self.screen.take_control(source, holder, controller)?;
        Ok(())
    }

    /// Release `holder`'s input on `source`, leaving the desktop view-only again
    /// and its capture path untouched.
    pub fn release_screen_control(
        &self,
        source: &str,
        holder: goble_screen_core::ControlHolder,
    ) -> anyhow::Result<()> {
        self.screen.release_control(source, holder)?;
        Ok(())
    }

    /// Send the user's own pointer click to `source`, taking the desktop first
    /// when nobody is driving it. The user's path is the screen pane's input;
    /// see [`Self::click_as`].
    pub fn click(&self, source: &str, x: u32, y: u32) -> anyhow::Result<()> {
        self.click_as(goble_screen_core::ControlHolder::User, source, x, y)
    }

    /// Type `text` into `source` as the user's own path. See [`Self::click`].
    pub fn type_text(&self, source: &str, text: &str) -> anyhow::Result<()> {
        self.type_text_as(goble_screen_core::ControlHolder::User, source, text)
    }

    /// Scroll `source` by `(dx, dy)` ticks as the user's own path. See
    /// [`Self::click`].
    pub fn scroll(&self, source: &str, dx: i32, dy: i32) -> anyhow::Result<()> {
        self.scroll_as(goble_screen_core::ControlHolder::User, source, dx, dy)
    }

    /// Click at `(x, y)` on `source` as `holder`, taking the desktop when
    /// nobody is driving it.
    ///
    /// The two input entry points of a desktop meet here: the app's own input
    /// (the screen pane, [`Self::click`]) and the agent's computer-use path,
    /// which asks as [`goble_screen_core::ControlHolder::Agent`]. Taking control
    /// first is what makes an opened desktop drivable — a remote source
    /// registers its capturer alone — and who may take it from whom is the
    /// registry's rule, not a caller's: the user's input takes the desktop back
    /// from the agent at any time, while the agent's input on a desktop the user
    /// drives fails with [`goble_screen_core::ScreenError::ControlHeld`] instead
    /// of silently doing nothing.
    pub fn click_as(
        &self,
        holder: goble_screen_core::ControlHolder,
        source: &str,
        x: u32,
        y: u32,
    ) -> anyhow::Result<()> {
        self.take_screen_control_when_unheld(source, holder)?;
        self.screen.click_as(holder, source, x, y)?;
        Ok(())
    }

    /// Type `text` into `source` as `holder`. See [`Self::click_as`].
    pub fn type_text_as(
        &self,
        holder: goble_screen_core::ControlHolder,
        source: &str,
        text: &str,
    ) -> anyhow::Result<()> {
        self.take_screen_control_when_unheld(source, holder)?;
        self.screen.type_text_as(holder, source, text)?;
        Ok(())
    }

    /// Scroll `source` by `(dx, dy)` ticks as `holder`. See [`Self::click_as`].
    pub fn scroll_as(
        &self,
        holder: goble_screen_core::ControlHolder,
        source: &str,
        dx: i32,
        dy: i32,
    ) -> anyhow::Result<()> {
        self.take_screen_control_when_unheld(source, holder)?;
        self.screen.scroll_as(holder, source, dx, dy)?;
        Ok(())
    }

    /// Take `source` for `holder` unless `holder` already drives it.
    ///
    /// Taking is skipped when the holder is the one driving — an input call by
    /// the holder must not re-register its controller — and otherwise left to
    /// [`Self::take_screen_control`], so the registry's rule decides a user take
    /// from the agent and refuses the agent's take from the user.
    fn take_screen_control_when_unheld(
        &self,
        source: &str,
        holder: goble_screen_core::ControlHolder,
    ) -> anyhow::Result<()> {
        if self.screen.control_holder(source) != Some(holder) {
            self.take_screen_control(source, holder)?;
        }
        Ok(())
    }

    /// Record `source` as an open desktop whose controller waits for a holder.
    ///
    /// This is the seam [`Self::open_remote_screen`] records a connected RDP
    /// client through, and the one any other screen source enters through:
    /// registering the capturer is the source's own job — `RdpRemoteSource`
    /// does it and hands its controller back — while the controller waits here,
    /// view-only, until [`Self::take_screen_control`] (or an input entry point,
    /// which takes first) registers it for a holder. The liveness is the
    /// client's own handle, so a desktop whose client has gone can be reported
    /// rather than drawn.
    ///
    /// The host's one-desktop rule is [`Self::open_remote_screen`]'s: it consults
    /// [`Self::remote_desktop_for_host`] before it connects. This only records
    /// what is there.
    pub fn record_open_desktop(
        &self,
        source: impl Into<String>,
        host: impl Into<String>,
        port: u16,
        controller: Arc<dyn goble_screen_core::ScreenController>,
        liveness: goble_screen_core::ClientLiveness,
    ) {
        let source = source.into();
        self.remote_desktops.lock().insert(
            source.clone(),
            RemoteDesktop {
                source,
                host: host.into(),
                port,
                controller,
                liveness,
            },
        );
    }

    /// The controller the source exposes to whoever opened it as a remote
    /// desktop.
    fn remote_controller(
        &self,
        source: &str,
    ) -> anyhow::Result<Arc<dyn goble_screen_core::ScreenController>> {
        self.remote_desktops
            .lock()
            .get(source)
            .map(|desktop| Arc::clone(&desktop.controller))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "source '{source}' was not opened as a remote desktop, so there is no \
                     controller to take"
                )
            })
    }

    /// The daemon the desktop service drives, as a [`DaemonClient`] so the GUI
    /// (a thin client) never reaches into daemon internals.
    pub fn daemon_client(&self) -> Arc<dyn goble_daemon_client::DaemonClient> {
        Arc::clone(&self.daemon)
    }

    /// Open the desktop a handoff names — the host's own if it already has one —
    /// and announce it as `screen:handoff` on `chat_id`.
    ///
    /// Both handoff paths land here: the local daemon's event, translated in
    /// [`super::translator`], and the one a paired worker relays, handled in
    /// [`super::worker_messages`]. One path means a desktop opened through a
    /// worker is found again exactly like a local one.
    pub(crate) fn hand_off_remote_screen(
        &self,
        chat_id: &str,
        config: goble_harness_types::RemoteScreenConfig,
    ) {
        match self.open_remote_screen(config) {
            Ok(source) => {
                self.add_log(format!("remote desktop {source} is open for the handoff"));
                self.emit(
                    "screen:handoff",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "source": source,
                    }),
                );
            }
            Err(e) => {
                self.add_log(format!("screen handoff failed: {e:#}"));
            }
        }
    }

    /// Rewind a settled session's transcript to keep `at` turns, dropping the
    /// tail. Reversibility operates on the recorded transcript, so it works with
    /// any harness the daemon drives.
    pub fn rewind(&self, session_id: &SessionId, at: usize) -> anyhow::Result<usize> {
        self.daemon.rewind(session_id, at)
    }

    /// Fork a fresh session from a checkpoint index, carrying the recorded prefix.
    pub fn fork(
        &self,
        session_id: &SessionId,
        at: usize,
        new_session_id: SessionId,
    ) -> anyhow::Result<SessionId> {
        self.daemon.fork(session_id, at, new_session_id)
    }

    /// Re-emit a session's recorded transcript from checkpoint `at` onward.
    pub fn replay(
        &self,
        session_id: &SessionId,
        at: usize,
    ) -> anyhow::Result<Vec<goble_daemon_protocol::DaemonEvent>> {
        self.daemon.replay(session_id, at)
    }

    /// List a session's per-turn checkpoints.
    pub fn checkpoints(&self, session_id: &SessionId) -> anyhow::Result<Vec<goble_daemon::Checkpoint>> {
        self.daemon.checkpoints(session_id)
    }

    /// Register an external (BYOH) harness so the daemon can drive it alongside
    /// the internal harness. Registrations are keyed by the harness's id;
    /// re-registering an id replaces it. The harness is any
    /// [`goble_harness_runtime::HarnessRuntime`], e.g. a `goble_harness_cli` CLI
    /// adapter wrapping an external binary.
    pub fn register_harness(&self, harness: Arc<dyn goble_harness_runtime::HarnessRuntime>) {
        self.daemon_state.register(harness);
    }

    pub fn set_event_bus(&self, bus: Arc<dyn EventBus>) {
        *self.event_bus.lock() = bus;
    }

    pub fn emit<T: Serialize>(&self, event: &str, payload: T) {
        emit_value(&**self.event_bus.lock(), event, payload);
    }
}
