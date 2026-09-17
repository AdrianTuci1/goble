//! State of the desktop service: the store, chats, workers, cluster identity,
//! credential vault, MCP servers and the embedded daemon it drives.
//!
//! One module per surface of the service: bootstrapping and the core accessors
//! ([`bootstrap`]), the daemon-event translator that feeds the UI ([`translator`]),
//! chat turns ([`turns`]), re-attaching a client to a conversation's session
//! ([`attach`]), chat storage ([`chats`]), agents, workflows and teams
//! ([`agents`]), worker pairing and routing ([`workers`]), cluster identity and
//! installs ([`cluster`]), the vault and access grants ([`vault`]), the
//! environment secret groups ([`environment`]), LLM settings
//! and provider resolution ([`llm`]), app-level settings ([`settings`]), MCP
//! servers ([`mcp`]) and the store-to-memory load ([`load`]). What the surfaces
//! share — the [`DesktopState`] record and the DTOs the app reads back — lives
//! here and in [`types`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use goble_core::agent::AgentId;
use goble_core::cluster_key::ClusterIdentity;
use goble_core::mcp_manager::McpManager;
use goble_core::store::Store;
use goble_core::vault::CredentialVault;
use goble_core::worker::WorkerId;
use goble_core::workflow::WorkflowId;
use goble_harness_types::{ProjectId, SessionId};
use goble_persistence::{Project, Session, Task};
use parking_lot::Mutex;

use crate::event_bus::EventBus;
use crate::thread_store::ThreadStore;
use crate::worker_manager::WorkerClient;

mod agents;
mod attach;
mod bootstrap;
mod chats;
mod cluster;
mod environment;
mod llm;
mod load;
mod mcp;
mod settings;
mod translator;
mod turns;
mod types;
mod vault;
mod worker_messages;
mod workers;

#[cfg(test)]
mod tests;

pub use attach::{AttachedSession, SessionAttach};
pub use bootstrap::RemoteDesktopInfo;
pub use types::{
    AgentInfo, Chat, ChatMessage, ClusterIdentityInfo, CommandProposedEvent, ExecutionInfo, Intent,
    IntentParams, LlmSetting, LogEntry, ReasoningEvent, ReasoningPhase, SubAgentFinishedEvent,
    SubAgentProgressEvent, SubAgentSpawnedEvent, TeamInfo, ThreadMessageSummary,
    ThreadReactionSummary, ThreadSummary, TokenUsageEvent, ToolCallEvent, VaultSecretInfo, WorkerConnection,
    WorkerInvite, WorkflowInfo,
};

pub struct DesktopState {
    store: Arc<Mutex<Store>>,
    workers: Arc<Mutex<HashMap<WorkerId, WorkerConnection>>>,
    chats: Arc<Mutex<Vec<Chat>>>,
    messages: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    agents: Arc<Mutex<HashMap<AgentId, AgentInfo>>>,
    workflows: Arc<Mutex<HashMap<WorkflowId, WorkflowInfo>>>,
    teams: Arc<Mutex<HashMap<String, TeamInfo>>>,
    /// Durable Project/Session/Task entities, seeded from the persistence layer
    /// on startup so the workspace's meta-information survives a restart.
    projects: Arc<Mutex<HashMap<ProjectId, Project>>>,
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    executions: Arc<Mutex<HashMap<String, ExecutionInfo>>>,
    vault: Arc<Mutex<CredentialVault>>,
    vault_passphrase: Mutex<Vec<u8>>,
    logs: Arc<Mutex<Vec<LogEntry>>>,
    clients: Arc<Mutex<HashMap<WorkerId, WorkerClient>>>,
    mcp_manager: McpManager,
    event_bus: Mutex<Arc<dyn EventBus>>,
    cluster_identity: Mutex<Option<ClusterIdentity>>,
    thread_store: Arc<ThreadStore>,
    config: parking_lot::Mutex<goble_core::config::GobleConfig>,
    /// The `config.toml` this state saves to. `None` on a state built with
    /// [`DesktopState::new`], which has no home of its own: `open_default` pins
    /// the located home, tests pin a temp file. Nothing resolves the user's real
    /// home on its own, so a state over a temp store cannot write into it.
    config_path: Mutex<Option<PathBuf>>,
    /// The `~/.goble/environment.toml` groups as last read, and why the last
    /// read failed. The file is the identity of a group; this is only the page's
    /// last view of it.
    environment: Mutex<environment::EnvironmentCache>,
    /// The daemon core (embedded): owns the harness registry and execution
    /// ledger. DesktopState is the composition root for the embedded daemon, so
    /// it seeds the registry and drives the daemon instead of constructing a
    /// `goble_core::harness::Harness` directly.
    daemon_state: Arc<goble_daemon::DaemonState>,
    /// GUI-side facade for the daemon: run/resume/cancel/introspection plus the
    /// event subscription the UI uses to render live state.
    daemon: Arc<dyn goble_daemon_client::DaemonClient>,
    /// Lazily spawned once, on the first harness turn: translates daemon wire
    /// events into the `chat:*` events the native UI listens for. Guarded by
    /// `translator_spawned` because `DesktopState::new` runs outside a tokio
    /// runtime (in the integration tests), so the task is only created when a
    /// runtime is current.
    translator_spawned: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Per-chat tool calls that are still running, keyed by chat id then call id.
    /// Updated from the daemon's tool lifecycle events; `chat:tool` carries the
    /// same record to the app, which overlays it on the persisted rows so a
    /// running call is visible before the turn ends.
    tool_in_flight: Arc<Mutex<HashMap<String, HashMap<String, ToolCallEvent>>>>,
    /// Screen capture+control registry, seeded with the local platform adapter
    /// on construction so the capturer/controller are reachable. The backend is
    /// platform-aware: macOS uses the real `screencapture` capturer, other
    /// platforms fall back to a synthetic adapter.
    screen: goble_screen_core::ScreenRegistry,
    /// Every remote desktop this state has open, keyed by the source id the
    /// registry holds it under — the one recorded source-id → host mapping, so
    /// "which desktop is this host's" is answered in one place rather than at a
    /// call site. A second handoff for a host reuses the desktop recorded for
    /// it instead of connecting again, and the record carries whether the RDP
    /// client behind it is still there, so a desktop whose client is gone is
    /// reported instead of serving the frame it left behind.
    ///
    /// The record holds the controller too: a remote desktop is view-only while
    /// nobody has taken it, so the registry holds its capturer and the
    /// controller waits here for [`DesktopState::take_screen_control`]. The
    /// local source is not here: its own adapter registers both halves.
    remote_desktops: Mutex<HashMap<String, bootstrap::RemoteDesktop>>,
}
