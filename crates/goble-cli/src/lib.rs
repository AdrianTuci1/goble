use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use futures::SinkExt;
use goble_core::agent::{AgentSpec, Trigger};
use goble_core::cluster_key::{ClusterIdentity, ClusterKey};
use goble_core::crypto::{generate_pairing_code, hash_pairing_code};
use goble_core::encrypted_wallet::IdentityWallet;
use goble_core::identity::ClusterRole;
use goble_core::protocol::DesktopMessage;
use goble_core::provision::{provision_worker, LocalTransport, ProvisionConfig, SshTransport};
use goble_core::snapshot::{LocalSnapshotProvider, SnapshotProvider};
use goble_core::store::Store;
use goble_core::tls::CertGenerator;
use goble_core::worker::{WorkerConfig, WorkerId};

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
    WorkerTag {
        id: String,
        tag: String,
    },
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
}

pub fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

pub async fn async_main() -> Result<()> {
    let args = Args::parse();
    let store = init_store().await?;

    match args.command {
        Command::WorkerAdd {
            name,
            host,
            username,
            port,
        } => {
            let config = WorkerConfig::new(&name, &host, &username)
                .with_pairing_code(generate_pairing_code());
            let config_port = port.unwrap_or(7878);
            let worker_id = config.id.clone();
            store.insert_worker(
                &worker_id.0,
                &name,
                Some(&format!("{}:{}", host, config_port)),
                "unpaired",
                None,
                &serde_json::to_string(&config)?,
                "",
                "",
            )?;
            println!(
                "added worker {} ({}) with pairing code {}",
                worker_id, name, config.pairing_code
            );
        }
        Command::WorkerList => {
            let workers = store.list_workers()?;
            for (id, name, host, status, _, _, _, _) in workers {
                println!("{}\t{}\t{}\t{}", id, name, host.unwrap_or_default(), status);
            }
        }
        Command::WorkerRemove { id } => {
            store.delete_worker(&id)?;
            println!("removed worker");
        }
        Command::WorkerTag { id, tag } => {
            let (_, _, _, config_json) = store
                .get_worker(&id)?
                .ok_or_else(|| anyhow::anyhow!("worker not found"))?;
            let mut config: WorkerConfig = serde_json::from_str(&config_json)?;
            if !config.tags.contains(&tag) {
                config.tags.push(tag.clone());
            }
            store.insert_worker(
                &config.id.0,
                &config.name,
                Some(&format!("{}:{}", config.host, config.port)),
                "tagged",
                None,
                &serde_json::to_string(&config)?,
                "",
                "",
            )?;
            println!("tagged worker {} with {}", id, tag);
        }
        Command::WorkerProvision {
            name,
            host,
            username,
            ssh_key,
            install_path,
            install_docker,
            install_hermes,
            install_crewai,
            local_test,
        }
        | Command::SetupWorker {
            name,
            host,
            username,
            ssh_key,
            install_path,
            install_docker,
            install_hermes,
            install_crewai,
            local_test,
        } => {
            do_provision(
                &store,
                name,
                host,
                username,
                ssh_key,
                install_path,
                install_docker,
                install_hermes,
                install_crewai,
                local_test,
            )?;
        }
        Command::Pair { code } => {
            let code = code.unwrap_or_else(generate_pairing_code);
            let hash = hash_pairing_code(&code, &[0u8; 16])?;
            println!("code: {}\nhash: {}", code, hash);
        }
        Command::ScheduleManage { action } => match action {
            ScheduleAction::List { worker, url, code } => {
                let code = code.unwrap_or_else(|| "00000000".to_string());
                send_to_worker(
                    &store,
                    &worker,
                    &url,
                    &code,
                    DesktopMessage::ListScheduledTasks,
                )
                .await?;
                println!("schedule list request sent");
            }
            ScheduleAction::Cancel {
                worker,
                url,
                task_id,
                code,
            } => {
                let code = code.unwrap_or_else(|| "00000000".to_string());
                send_to_worker(
                    &store,
                    &worker,
                    &url,
                    &code,
                    DesktopMessage::CancelScheduledTask { task_id },
                )
                .await?;
                println!("schedule cancel request sent");
            }
        },
        Command::Run {
            worker,
            url,
            name,
            prompt,
            code,
        } => {
            let agent = AgentSpec::new(&name, &prompt);
            let trace_id = uuid::Uuid::new_v4().to_string();
            let code = code.unwrap_or_else(|| "00000000".to_string());
            send_to_worker(
                &store,
                &worker,
                &url,
                &code,
                DesktopMessage::RunAgent {
                    trace_id,
                    agent_id: agent.id.clone(),
                    spec: agent,
                    mcp_servers: vec![],
                },
            )
            .await?;
            println!("agent dispatched");
        }
        Command::Schedule {
            worker,
            url,
            agent_id,
            cron,
            heartbeat,
        } => {
            let trigger = if let Some(expr) = cron {
                Trigger::Cron { expression: expr }
            } else if let Some(seconds) = heartbeat {
                Trigger::Heartbeat {
                    interval_seconds: seconds,
                }
            } else {
                Trigger::Manual
            };
            let code = "00000000".to_string();
            send_to_worker(
                &store,
                &worker,
                &url,
                &code,
                DesktopMessage::ScheduleAgent {
                    agent_id: goble_core::agent::AgentId(agent_id),
                    trigger,
                    mcp_servers: vec![],
                },
            )
            .await?;
            println!("schedule sent");
        }
        Command::Secret { action } => match action {
            SecretAction::Set {
                worker,
                url,
                name,
                value,
                code,
            } => {
                let code = code.unwrap_or_else(|| "00000000".to_string());
                send_to_worker(
                    &store,
                    &worker,
                    &url,
                    &code,
                    DesktopMessage::SetVaultSecret {
                        name,
                        value: value.into_bytes(),
                    },
                )
                .await?;
                println!("secret set request sent");
            }
            SecretAction::Get {
                worker,
                url,
                name,
                code,
            } => {
                let code = code.unwrap_or_else(|| "00000000".to_string());
                send_to_worker(
                    &store,
                    &worker,
                    &url,
                    &code,
                    DesktopMessage::GetVaultSecret { name },
                )
                .await?;
                println!("secret get request sent");
            }
        },
        Command::Snapshot { action } => match action {
            SnapshotAction::List { dir } => {
                let provider = LocalSnapshotProvider::new(dir);
                for entry in provider.list_snapshots()? {
                    println!("{}\t{}\t{} bytes", entry.key, entry.created_at, entry.size);
                }
            }
            SnapshotAction::Restore {
                dir,
                store,
                cluster_key,
            } => {
                let key: ClusterKey = cluster_key.parse()?;
                let provider = LocalSnapshotProvider::new(dir);
                let snapshots = provider.list_snapshots()?;
                let latest = snapshots
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("no snapshots found"))?;
                let snapshot = provider.download_snapshot(&latest.key)?;
                let db = Store::open(store)?;
                snapshot.restore_into_store(&db, &key)?;
                println!("restored from {}", latest.key);
            }
            SnapshotAction::Trigger { worker, url, code } => {
                let code = code.unwrap_or_else(|| "00000000".to_string());
                send_to_worker(
                    &store,
                    &worker,
                    &url,
                    &code,
                    DesktopMessage::TriggerSnapshot,
                )
                .await?;
                println!("snapshot trigger request sent");
            }
        },
        Command::Identity { action } => match action {
            IdentityAction::Create {
                name,
                passphrase,
                out,
            } => {
                let device_id = format!("cli-{}", uuid::Uuid::new_v4());
                let identity = ClusterIdentity::generate(&name, &device_id, ClusterRole::Owner)?;
                let wallet = IdentityWallet::from(&identity);
                let sealed = wallet.seal(passphrase.as_bytes())?;
                let json = serde_json::to_string(&sealed)?;
                std::fs::write(&out, json)?;
                println!(
                    "created identity wallet for '{}' at {}",
                    name,
                    out.display()
                );
                println!("cluster key: {}", identity.export_key());
            }
            IdentityAction::Export { passphrase, out } => {
                let wallet = store
                    .get_cluster_wallet()?
                    .ok_or_else(|| anyhow::anyhow!("no cluster wallet in store"))?;
                let plaintext = wallet.open(passphrase.as_bytes())?;
                let identity: IdentityWallet = serde_json::from_slice(&plaintext)
                    .context("wallet does not contain a valid IdentityWallet")?;
                let sealed = identity.seal(passphrase.as_bytes())?;
                let json = serde_json::to_string(&sealed)?;
                std::fs::write(&out, json)?;
                println!("exported identity wallet to {}", out.display());
            }
            IdentityAction::Restore { passphrase, wallet } => {
                let json = std::fs::read_to_string(&wallet)?;
                let sealed: goble_core::encrypted_wallet::EncryptedWallet =
                    serde_json::from_str(&json)?;
                let plaintext = sealed.open(passphrase.as_bytes())?;
                let identity: IdentityWallet = serde_json::from_slice(&plaintext)
                    .context("wallet does not contain a valid IdentityWallet")?;
                let resealed = identity.seal(passphrase.as_bytes())?;
                store.set_cluster_wallet(&resealed)?;
                println!(
                    "restored identity wallet from {} into store",
                    wallet.display()
                );
            }
        },
        Command::Device { action } => match action {
            DeviceAction::Restore {
                from_snapshot,
                cluster_key,
                passphrase,
                device_id,
                device_name,
            } => {
                let key = ClusterKey::from_base64(&cluster_key)?;
                let provider = LocalSnapshotProvider::new(&from_snapshot);
                let (wallet, identity) =
                    goble_core::device_transfer::DeviceTransfer::restore_from_snapshot(
                        &provider,
                        &WorkerId::generate(),
                        &key,
                        passphrase.as_bytes(),
                        &device_id,
                        &device_name,
                        ClusterRole::Admin,
                    )?;
                let sealed = wallet.seal(passphrase.as_bytes())?;
                store.set_cluster_wallet(&sealed)?;
                println!(
                    "joined cluster '{}' as device {}",
                    wallet.cluster_name, device_id
                );
                println!("device certificate serial: {}", identity.serial());
            }
        },
        Command::Cluster { action } => match action {
            ClusterAction::HelmInstall {
                name,
                namespace,
                replicas,
                passphrase,
                provider,
                endpoint,
                bucket,
                access_key_id,
                secret_access_key,
                region,
                interval_seconds,
                local_chart,
            } => {
                let sealed = store
                    .get_cluster_wallet()?
                    .ok_or_else(|| anyhow::anyhow!("no cluster wallet in store; create or restore identity first"))?;
                let plaintext = sealed.open(passphrase.as_bytes())?;
                let wallet: IdentityWallet = serde_json::from_slice(&plaintext)
                    .context("wallet does not contain a valid IdentityWallet")?;
                let identity = wallet.to_cluster_identity("cli-cluster-command", ClusterRole::Admin)?;
                let worker_id = "goblin-cluster".to_string();
                let bundle = identity
                    .ca
                    .sign_worker_bundle(&worker_id, &wallet.cluster_name, 365)?;
                let bundle_json = serde_json::to_string(&bundle)?;
                let bundle_b64 = base64::engine::general_purpose::STANDARD.encode(bundle_json);
                let cluster_key_b64 = identity.export_key();

                let mut helm_args = vec![
                    format!("helm install {} ", name),
                    if let Some(chart) = local_chart {
                        format!("{} ", chart.display())
                    } else {
                        "goble/goblin-cluster ".to_string()
                    },
                    format!("--namespace {} --create-namespace ", namespace),
                    format!("--set replicas={} ", replicas),
                    format!("--set workerBundle={} ", bundle_b64),
                    format!("--set clusterKey={} ", cluster_key_b64),
                    format!("--set snapshot.enabled=true "),
                    format!("--set snapshot.provider={} ", provider),
                    format!("--set snapshot.intervalSeconds={} ", interval_seconds),
                    format!("--set snapshot.region={} ", region),
                ];
                if let Some(endpoint) = endpoint {
                    helm_args.push(format!("--set snapshot.endpoint={} ", endpoint));
                }
                if let Some(bucket) = bucket {
                    helm_args.push(format!("--set snapshot.bucket={} ", bucket));
                }
                if let Some(access_key_id) = access_key_id {
                    helm_args.push(format!("--set snapshot.accessKeyId={} ", access_key_id));
                }
                if let Some(secret_access_key) = secret_access_key {
                    helm_args.push(format!("--set snapshot.secretAccessKey={} ", secret_access_key));
                }
                helm_args.push("\n".to_string());
                println!("Run the following command in a cluster with Helm configured:");
                println!("{}", helm_args.join(""));
            }
        },
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn do_provision(
    store: &Store,
    name: String,
    host: String,
    username: String,
    ssh_key: Option<PathBuf>,
    install_path: String,
    install_docker: bool,
    install_hermes: bool,
    install_crewai: bool,
    local_test: bool,
) -> Result<()> {
    let pairing_code = generate_pairing_code();
    let pairing_hash = hash_pairing_code(&pairing_code, &[0u8; 16])?;
    let worker_id = WorkerId::generate();

    let ca = CertGenerator::generate_ca()?;
    let server = CertGenerator::generate_server(&ca, &host)?;
    let desktop = CertGenerator::generate_client(&ca, &worker_id.0)?;
    let worker_bundle = goble_core::provision::WorkerBundle {
        worker_id: worker_id.0.clone(),
        cert_pem: server.cert_pem.clone(),
        key_pem: server.key_pem.clone(),
        ca_cert_pem: ca.cert_pem.clone(),
        cluster_name: "goble".to_string(),
    };

    let config = ProvisionConfig {
        worker_id: worker_id.0.clone(),
        name: name.clone(),
        install_path: install_path.clone(),
        workspace_root: "/var/goblin/workspaces".to_string(),
        pairing_code_hash: pairing_hash.clone(),
        install_docker,
        install_hermes,
        install_crewai,
        goblin_binary: std::env::current_exe()?
            .parent()
            .map(|p| p.join("goblin"))
            .unwrap_or_else(|| PathBuf::from("goblin")),
        worker_bundle,
    };

    if local_test {
        let tmp = tempfile::TempDir::new()?;
        let transport = LocalTransport::new(tmp.path());
        provision_worker(&transport, &config)?;
    } else {
        let transport = SshTransport::new(&host, &username, ssh_key);
        provision_worker(&transport, &config)?;
    }

    let worker_config = WorkerConfig::new(&name, &host, &username)
        .with_pairing_code(&pairing_code)
        .with_worker_bundle(config.worker_bundle.clone())
        .with_desktop_identity(goble_core::identity::Identity::from_pem(
            desktop.cert_pem,
            desktop.key_pem,
        )?);

    store.insert_worker(
        &worker_id.0,
        &name,
        Some(&format!("{}:8787", host)),
        "provisioned",
        None,
        &serde_json::to_string(&worker_config)?,
        "",
        "",
    )?;

    println!(
        "provisioned worker {} ({}) on {} with pairing code {} and mTLS bundle",
        worker_id.0, name, host, pairing_code
    );
    Ok(())
}

async fn init_store() -> Result<Store> {
    let path = dirs::config_dir()
        .map(|p| p.join("goble"))
        .unwrap_or_else(|| PathBuf::from(".goble"));
    std::fs::create_dir_all(&path)?;
    Store::open(path.join("store.db"))
}

async fn send_to_worker(
    store: &Store,
    worker_id: &str,
    url: &str,
    pairing_code: &str,
    msg: DesktopMessage,
) -> Result<()> {
    let config = store
        .get_worker(worker_id)?
        .and_then(|(_, _, _, cfg)| serde_json::from_str::<WorkerConfig>(&cfg).ok());

    let use_mtls = config
        .as_ref()
        .map(|c| c.worker_bundle.is_some())
        .unwrap_or(false);
    let url = if use_mtls {
        let cfg = config.as_ref().unwrap();
        let host = if url.is_empty() { &cfg.host } else { url };
        if host.starts_with("wss://") {
            host.to_string()
        } else if host.starts_with("ws://") {
            host.replacen("ws://", "wss://", 1)
        } else {
            format!("wss://{}/ws", host.trim_end_matches("/ws"))
        }
    } else if url.is_empty() {
        config
            .as_ref()
            .map(|c| c.websocket_url())
            .unwrap_or_else(|| format!("ws://{}/ws", worker_id))
    } else {
        url.to_string()
    };

    let mut ws_stream = if use_mtls {
        let cfg = config.as_ref().unwrap();
        let bundle = cfg.worker_bundle.as_ref().unwrap();
        let desktop_identity = cfg
            .desktop_identity
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("worker config missing desktop identity for mTLS"))?;
        let tls_config = bundle.client_config(desktop_identity)?;
        let connector = tokio_tungstenite::Connector::Rustls(Arc::new(tls_config));
        let (stream, _resp) =
            tokio_tungstenite::connect_async_tls_with_config(&url, None, false, Some(connector))
                .await?;
        stream
    } else {
        let (stream, _resp) = tokio_tungstenite::connect_async(&url).await?;
        stream
    };

    let pair = DesktopMessage::PairRequest {
        worker_id: WorkerId(worker_id.to_string()),
        pairing_code_hash: if use_mtls {
            None
        } else {
            Some(hash_pairing_code(pairing_code, &[0u8; 16])?)
        },
    };
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&pair)?.into(),
        ))
        .await?;
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&msg)?.into(),
        ))
        .await?;
    Ok(())
}
