use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "goble-cli")]
#[command(about = "Goble command-line interface")]
pub struct Args {
    #[arg(long, default_value = "~/.config/goble/goble.toml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a new worker profile.
    WorkerAdd {
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        host: String,
        #[arg(short, long)]
        username: String,
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// List configured workers.
    WorkerList,
    /// Remove a worker profile.
    WorkerRemove { id: String },
    /// Add a tag to a worker profile.
    WorkerTag { id: String, tag: String },
    /// Provision and deploy a Goblin worker on a remote VPS.
    WorkerProvision {
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        host: String,
        #[arg(short, long)]
        username: String,
        #[arg(short, long)]
        ssh_key: Option<PathBuf>,
        #[arg(short, long, default_value = "/opt/goblin")]
        install_path: String,
        #[arg(long, default_value = "false")]
        local_test: bool,
    },
    /// Interactive setup for a new Goblin worker (alias for worker-provision).
    #[command(name = "setup-worker")]
    SetupWorker {
        #[arg(short, long)]
        name: String,
        #[arg(short = 'o', long)]
        host: String,
        #[arg(short, long)]
        username: String,
        #[arg(short, long)]
        ssh_key: Option<PathBuf>,
        #[arg(short, long, default_value = "/opt/goblin")]
        install_path: String,
        #[arg(long, default_value = "false")]
        local_test: bool,
    },
    /// Generate a pairing code hash for a worker.
    Pair { code: Option<String> },
    /// Manage scheduled tasks on a worker.
    ScheduleManage {
        #[command(subcommand)]
        action: ScheduleAction,
    },
    /// Run an agent on a worker via WebSocket.
    Run {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        prompt: String,
        #[arg(short, long)]
        code: Option<String>,
    },
    /// Schedule an agent on a worker.
    Schedule {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        agent_id: String,
        #[arg(short, long)]
        cron: Option<String>,
        #[arg(short, long)]
        heartbeat: Option<u64>,
    },
    /// Manage encrypted secrets on a worker.
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Manage worker snapshots.
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },
    /// Manage devices (join from snapshot).
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },
    /// Manage the cluster identity wallet.
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },
    /// Generate a Kubernetes Helm install command for a Goblin worker cluster.
    Cluster {
        #[command(subcommand)]
        action: ClusterAction,
    },
    /// Set up Remote (a sandbox provider or a gateway) and the hub.
    Setup {
        /// The config file to read and write; defaults to ~/.goble/config.toml.
        #[arg(long, global = true, value_name = "PATH")]
        config: Option<PathBuf>,
        #[command(subcommand)]
        action: SetupAction,
    },
    /// Control a running Goble window over its Unix control socket.
    #[cfg(unix)]
    Ctl {
        /// Print the typed result as JSON (and errors as a structured object).
        #[arg(long, global = true)]
        json: bool,
        /// Control socket to call; defaults to $GOBLE_CONTROL_SOCKET or
        /// $HOME/.goble/control.sock.
        #[arg(long, global = true, value_name = "PATH")]
        socket: Option<PathBuf>,
        #[command(subcommand)]
        action: CtlAction,
    },
}

/// The `goble ctl` verbs. Each one is one typed control command.
#[cfg(unix)]
#[derive(Subcommand, Debug)]
pub enum CtlAction {
    /// List every open pane.
    ListPanes,
    /// Open a new pane.
    NewPane {
        /// Workspace tab index; defaults to the active space.
        #[arg(long)]
        space_id: Option<usize>,
        /// Working directory for a terminal pane.
        #[arg(long)]
        cwd: Option<String>,
        /// Pane kind.
        #[arg(long, value_enum, default_value_t = PaneKindArg::Terminal)]
        kind: PaneKindArg,
    },
    /// Split a pane.
    SplitPane {
        pane_id: u64,
        /// Split direction: horizontal is left/right, vertical is top/bottom.
        #[arg(long, value_enum, default_value_t = AxisArg::Horizontal)]
        axis: AxisArg,
    },
    /// Close a pane.
    ClosePane { pane_id: u64 },
    /// Focus a pane.
    FocusPane { pane_id: u64 },
    /// Write text to a pane's terminal.
    SendInput {
        pane_id: u64,
        text: String,
        /// Press Enter after the text.
        #[arg(long)]
        enter: bool,
    },
    /// Resize a pane's terminal grid.
    ResizePane {
        pane_id: u64,
        cols: u16,
        rows: u16,
    },
    /// List workspace tabs.
    ListSpaces,
    /// Switch to a workspace tab.
    SwitchSpace { space_id: usize },
    /// Report pane agent state, or one pane with --pane-id.
    AgentState {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// Send a prompt to a pane's agent.
    PromptAgent { pane_id: u64, text: String },
    /// List the global agent inbox.
    Inbox,
    /// Acknowledge one inbox item, or all of them.
    AckInbox {
        #[arg(long, conflicts_with = "all")]
        id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Read a space's layout, or set it with --kind.
    Layout {
        space_id: usize,
        #[arg(long, value_enum)]
        kind: Option<LayoutKindArg>,
    },
    /// List PTY daemon sessions.
    PtyList,
    /// Attach to a PTY daemon session.
    PtyAttach { session_id: String },
    /// Detach from a PTY daemon session.
    PtyDetach { session_id: String },
    /// Kill a PTY daemon session.
    PtyKill { session_id: String },
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PaneKindArg {
    Chat,
    Terminal,
}

#[cfg(unix)]
impl From<PaneKindArg> for goble_control::PaneKind {
    fn from(kind: PaneKindArg) -> Self {
        match kind {
            PaneKindArg::Chat => Self::Chat,
            PaneKindArg::Terminal => Self::Terminal,
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AxisArg {
    Horizontal,
    Vertical,
}

#[cfg(unix)]
impl From<AxisArg> for goble_control::SplitAxis {
    fn from(axis: AxisArg) -> Self {
        match axis {
            AxisArg::Horizontal => Self::Horizontal,
            AxisArg::Vertical => Self::Vertical,
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LayoutKindArg {
    Single,
    Horizontal,
    Vertical,
    Bsp,
    MasterStack,
    Floating,
    Carousel,
}

#[cfg(unix)]
impl From<LayoutKindArg> for goble_control::LayoutKind {
    fn from(kind: LayoutKindArg) -> Self {
        match kind {
            LayoutKindArg::Single => Self::Single,
            LayoutKindArg::Horizontal => Self::Horizontal,
            LayoutKindArg::Vertical => Self::Vertical,
            LayoutKindArg::Bsp => Self::Bsp,
            LayoutKindArg::MasterStack => Self::MasterStack,
            LayoutKindArg::Floating => Self::Floating,
            LayoutKindArg::Carousel => Self::Carousel,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum DeviceAction {
    /// Restore (or join) this device from an encrypted snapshot.
    Restore {
        #[arg(short, long)]
        from_snapshot: PathBuf,
        #[arg(short, long)]
        cluster_key: String,
        #[arg(short, long)]
        passphrase: String,
        #[arg(short = 'i', long, default_value = "restored-device")]
        device_id: String,
        #[arg(short = 'm', long, default_value = "Restored Device")]
        device_name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleAction {
    /// List scheduled tasks on a worker.
    List {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        code: Option<String>,
    },
    /// Cancel a scheduled task on a worker.
    Cancel {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        task_id: String,
        #[arg(short, long)]
        code: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SecretAction {
    /// Set or update an encrypted secret on a worker.
    Set {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        value: String,
        #[arg(short, long)]
        code: Option<String>,
    },
    /// Retrieve an encrypted secret from a worker.
    Get {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        code: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SnapshotAction {
    /// List snapshots stored locally or in a configured directory.
    List {
        #[arg(short, long)]
        dir: PathBuf,
    },
    /// Restore a local store from the latest snapshot.
    Restore {
        #[arg(short, long)]
        dir: PathBuf,
        #[arg(short, long)]
        store: PathBuf,
        #[arg(short, long)]
        cluster_key: String,
    },
    /// Ask a worker to upload a snapshot immediately.
    Trigger {
        #[arg(short, long)]
        worker: String,
        #[arg(short, long)]
        url: String,
        #[arg(short, long)]
        code: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ClusterAction {
    /// Print a helm install command for a Goblin worker cluster.
    HelmInstall {
        /// Helm release name.
        #[arg(short, long, default_value = "goblin")]
        name: String,
        /// Kubernetes namespace.
        #[arg(long, default_value = "goblin")]
        namespace: String,
        /// Number of worker replicas.
        #[arg(short, long, default_value = "3")]
        replicas: u32,
        /// Passphrase to decrypt the local identity wallet.
        #[arg(short, long)]
        passphrase: String,
        /// Snapshot provider: local, s3, r2, b2, minio.
        #[arg(long, default_value = "local")]
        provider: String,
        /// S3-compatible endpoint (e.g. R2 URL). Required for s3/r2/minio providers.
        #[arg(long)]
        endpoint: Option<String>,
        /// Snapshot bucket name.
        #[arg(long)]
        bucket: Option<String>,
        /// Snapshot access key id.
        #[arg(long)]
        access_key_id: Option<String>,
        /// Snapshot secret access key.
        #[arg(long)]
        secret_access_key: Option<String>,
        /// Snapshot region.
        #[arg(long, default_value = "auto")]
        region: String,
        /// Snapshot interval in seconds.
        #[arg(long, default_value = "3600")]
        interval_seconds: u64,
        /// Use a local chart path instead of the remote repo.
        #[arg(long)]
        local_chart: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum IdentityAction {
    /// Create a new cluster identity wallet.
    Create {
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        passphrase: String,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Export the cluster identity wallet from this device store to a file.
    Export {
        #[arg(short, long)]
        passphrase: String,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Restore a cluster identity wallet from a file into this device store.
    Restore {
        #[arg(short, long)]
        passphrase: String,
        #[arg(short, long)]
        wallet: PathBuf,
    },
    /// Rotate all worker certificates: revoke old ones and issue new bundles.
    RotateWorkerCerts {
        #[arg(short, long)]
        passphrase: String,
        #[arg(short, long, default_value = "365")]
        days: u64,
    },
}

/// The `goble setup` verbs: the agent-led setup conversation's write surface.
///
/// Every verb reads the same file the app reads, writes through
/// [`goble_core::config::GobleConfig`], keeps the file's header and sections it
/// does not touch, and reports what is still missing rather than inventing it.
#[derive(Subcommand, Debug)]
pub enum SetupAction {
    /// Report what Remote and the hub have, and what is missing.
    Status {
        /// Print the report as JSON, for a caller that reads it.
        #[arg(long)]
        json: bool,
    },
    /// Write a `[remote_agents.<name>]` table.
    Remote {
        /// The table's name: what the user will call this agent.
        #[arg(long)]
        name: String,
        /// The sandbox provider behind the table.
        #[arg(long, value_enum)]
        provider: SetupProviderArg,
        /// The provider's API key (e2b, daytona, box, or a Cloud Run bearer
        /// token).
        #[arg(long)]
        api_key: Option<String>,
        /// Modal's token id.
        #[arg(long)]
        token_id: Option<String>,
        /// Modal's token secret.
        #[arg(long)]
        token_secret: Option<String>,
        /// A provider endpoint override.
        #[arg(long)]
        api_url: Option<String>,
        /// The image that carries goblin-worker.
        #[arg(long)]
        image: Option<String>,
        /// The provider region (Cloud Run: the location).
        #[arg(long)]
        region: Option<String>,
        /// The sandbox environment (Cloud Run: the GCP project id).
        #[arg(long)]
        environment: Option<String>,
        /// Write `enabled = false`, so the table is kept but not offered.
        #[arg(long)]
        disabled: bool,
    },
    /// Point `[remote]` at a gateway, or back at the sandbox platforms.
    Gateway {
        /// The gateway's public URL.
        #[arg(long)]
        url: Option<String>,
        /// The worker token the gateway was started with.
        #[arg(long)]
        token: Option<String>,
        /// Go back to `mode = "platform"`, keeping the URL and token.
        #[arg(long, conflicts_with_all = ["url", "token"])]
        platform: bool,
    },
    /// Write the hub's settings to ~/.goble/hub.toml.
    Hub {
        /// The SQLite file the run ledger and audit chain live in.
        #[arg(long)]
        database: Option<String>,
        /// The relay the hub dials to reach workers.
        #[arg(long)]
        relay_url: Option<String>,
        /// The bearer token the hub's API accepts.
        #[arg(long)]
        token: Option<String>,
        /// The directory holding `.goble/workflows/*.toml`.
        #[arg(long)]
        workflows: Option<String>,
    },
    /// Print the brief a setup conversation starts from.
    Brief {
        /// Whether the brief covers Remote or the hub.
        #[arg(long, value_enum, default_value_t = SetupTargetArg::Remote)]
        target: SetupTargetArg,
    },
}

/// The providers `setup remote` can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SetupProviderArg {
    Modal,
    E2b,
    Daytona,
    Box,
    #[value(name = "cloud_run")]
    CloudRun,
    Docker,
    Ssh,
}

impl From<SetupProviderArg> for goble_core::config::RemoteAgentProvider {
    fn from(provider: SetupProviderArg) -> Self {
        use goble_core::config::RemoteAgentProvider;
        match provider {
            SetupProviderArg::Modal => RemoteAgentProvider::Modal,
            SetupProviderArg::E2b => RemoteAgentProvider::E2b,
            SetupProviderArg::Daytona => RemoteAgentProvider::Daytona,
            SetupProviderArg::Box => RemoteAgentProvider::Box,
            SetupProviderArg::CloudRun => RemoteAgentProvider::CloudRun,
            SetupProviderArg::Docker => RemoteAgentProvider::Docker,
            SetupProviderArg::Ssh => RemoteAgentProvider::Ssh,
        }
    }
}

/// Which setup `setup brief` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SetupTargetArg {
    Remote,
    Hub,
}

impl From<SetupTargetArg> for goble_core::config::SetupTarget {
    fn from(target: SetupTargetArg) -> Self {
        match target {
            SetupTargetArg::Remote => goble_core::config::SetupTarget::Remote,
            SetupTargetArg::Hub => goble_core::config::SetupTarget::Hub,
        }
    }
}
