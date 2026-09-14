use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
        #[arg(long)]
        install_docker: bool,
        #[arg(long)]
        install_hermes: bool,
        #[arg(long)]
        install_crewai: bool,
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
        #[arg(long)]
        install_docker: bool,
        #[arg(long)]
        install_hermes: bool,
        #[arg(long)]
        install_crewai: bool,
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
