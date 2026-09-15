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
        })
    }

    pub fn thread_store(&self) -> Arc<ThreadStore> {
        Arc::clone(&self.thread_store)
    }

    /// The screen capture+control registry, seeded with the local adapter.
    pub fn screen_registry(&self) -> &goble_screen_core::ScreenRegistry {
        &self.screen
    }

    /// Open a live remote desktop (xrdp / RDP) for `config` and register it into
    /// the screen registry under a derived source id. This is the host side of a
    /// harness [`ScreenHandoff`](goble_daemon_protocol::DaemonEvent::ScreenHandoff):
    /// the GUI then lists the new source and routes broadcast/computer-use input
    /// to it through the registry, identically to the local adapter.
    ///
    /// Returns the registered source id. Without the `remote-screen` feature this
    /// fails with a clear message instead of silently doing nothing.
    pub fn open_remote_screen(
        &self,
        config: goble_harness_types::RemoteScreenConfig,
    ) -> anyhow::Result<String> {
        #[cfg(feature = "remote-screen")]
        {
            let source = format!("remote-xrdp:{}:{}", config.host, config.port);
            let remote = goble_screen_sdk::RemoteConfig {
                host: config.host,
                port: config.port,
                username: config.username,
                password: config.password,
                width: config.width,
                height: config.height,
            };
            goble_screen_sdk::RdpRemoteSource::connect(&source, remote, &self.screen)?;
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

    /// The daemon the desktop service drives, as a [`DaemonClient`] so the GUI
    /// (a thin client) never reaches into daemon internals.
    pub fn daemon_client(&self) -> Arc<dyn goble_daemon_client::DaemonClient> {
        Arc::clone(&self.daemon)
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
