use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use goble_core::cluster_key::ClusterKey;
use goble_core::provision::WorkerBundle;
use goble_core::snapshot::LocalSnapshotProvider;
use goble_core::worker::{WorkerId, WorkerStatus};
use serde::Serialize;
use tracing_subscriber::EnvFilter;

pub mod agent_runtime;
pub mod file_vault;
pub mod harness_runner;
pub mod llm_factory;
pub mod mcp;
pub mod pairing;
pub mod runner;
pub mod scheduler;
pub mod snapshot_runner;
pub mod state;
pub mod task_store;
pub mod websocket;

#[derive(Parser, Debug)]
#[command(name = "goblin")]
struct Args {
    #[arg(long, default_value = "0.0.0.0:8787")]
    bind: String,
    #[arg(
        long,
        env = "GOBLIN_WORKSPACE_ROOT",
        default_value = "/tmp/goblin/workspaces"
    )]
    workspace_root: std::path::PathBuf,
    #[arg(long, env = "GOBLIN_WORKER_ID")]
    worker_id: Option<String>,
    #[arg(long, env = "GOBLIN_TLS_BUNDLE")]
    tls_bundle: Option<PathBuf>,
    #[arg(
        long,
        env = "GOBLIN_TASK_STORE",
        default_value = "/var/goblin/tasks.db"
    )]
    task_store: std::path::PathBuf,
    #[arg(
        long,
        env = "GOBLIN_VAULT_PATH",
        default_value = "/var/goblin/vault.json"
    )]
    vault_path: std::path::PathBuf,
    #[arg(long, env = "GOBLIN_DAEMON")]
    daemon: bool,
    #[arg(long, env = "GOBLIN_PID_FILE", default_value = "/var/run/goblin.pid")]
    pid_file: PathBuf,
    #[arg(long, env = "GOBLIN_LOG_FILE", default_value = "/var/log/goblin.log")]
    log_file: PathBuf,
    #[arg(long, env = "GOBLIN_CLUSTER_KEY")]
    cluster_key: Option<String>,
    #[arg(long, env = "GOBLIN_SNAPSHOT_DIR")]
    snapshot_dir: Option<std::path::PathBuf>,
    #[arg(long, env = "GOBLIN_SNAPSHOT_INTERVAL_SECONDS", default_value = "300")]
    snapshot_interval_seconds: u64,
}

static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

#[derive(Debug, Serialize, Clone)]
struct HealthReport {
    worker_id: String,
    status: WorkerStatus,
    paired: bool,
    uptime_seconds: u64,
    load: u8,
    active_traces: usize,
    scheduled_tasks: usize,
    version: &'static str,
}

#[derive(Debug, Serialize, Clone)]
struct PlatformInfo {
    os: String,
    arch: String,
    family: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    START_TIME.set(Instant::now()).ok();

    let args = Args::parse();

    if args.daemon {
        daemonize::daemonize(&args.pid_file, &args.log_file)?;
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let worker_id = match args.worker_id {
        Some(id) => WorkerId(id),
        None => WorkerId::generate(),
    };

    let workspace_root = args.workspace_root.clone();
    let state = state::AppState::new(worker_id.clone());
    {
        let mut config = state.config.lock();
        config.workspace_root = args.workspace_root.clone();
        config.llm_provider = std::env::var("LLM_PROVIDER").ok();
        config.llm_model = std::env::var("LLM_MODEL").ok();
        config.llm_base_url = std::env::var("LLM_BASE_URL").ok();
    }

    state.file_vault.lock().set_path(args.vault_path.clone());
    // Best-effort vault load with empty passphrase; client can unlock later.
    let _ = state.load_vault(b"");
    std::fs::create_dir_all(&workspace_root)?;
    state.set_store_path(workspace_root.join("worker.db"))?;

    if let Some(key_str) = args.cluster_key {
        let cluster_key: ClusterKey = key_str.parse().context("invalid cluster key")?;
        state.set_cluster_key(cluster_key.clone());
        if let Some(snapshot_dir) = args.snapshot_dir {
            std::fs::create_dir_all(&snapshot_dir)?;
            let provider = Arc::new(LocalSnapshotProvider::new(snapshot_dir));
            state.set_snapshot_provider(provider.clone());
            let runner = snapshot_runner::SnapshotRunner::new(
                state.clone(),
                provider,
                cluster_key,
                Duration::from_secs(args.snapshot_interval_seconds),
            );
            if runner.restore_if_empty()? {
                tracing::info!("restored worker state from snapshot");
            }
            runner.start();
        }
    }

    let task_store = task_store::TaskStore::open(args.task_store)?;
    let scheduler = Arc::new(scheduler::Scheduler::new_with_default_runner(
        state.clone(),
        task_store,
    ));
    let scheduler_for_state = Arc::clone(&scheduler);
    scheduler.start_loop(Duration::from_secs(5));
    state.set_scheduler(scheduler_for_state);

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/health", get(health_handler))
        .route("/platform", get(platform_handler))
        .route("/mcp", get(mcp::list_mcp_handler))
        .route("/pair", post(pairing::pair_handler))
        .route("/status", get(pairing::status_handler))
        .route("/ws", get(websocket::ws_handler))
        .with_state(state.clone());

    if let Some(bundle_path) = args.tls_bundle {
        let bundle_json = tokio::fs::read_to_string(&bundle_path).await?;
        let bundle: WorkerBundle = serde_json::from_str(&bundle_json)?;
        let rustls_config = RustlsConfig::from_config(Arc::new(bundle.server_config()?));
        state.set_worker_bundle(bundle);
        tracing::info!("goblin listening with mTLS on {}", args.bind);
        axum_server::bind_rustls(args.bind.parse()?, rustls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(&args.bind).await?;
        tracing::info!("goblin listening on {}", args.bind);
        axum::serve(listener, app).await?;
    }

    Ok(())
}

async fn root_handler() -> &'static str {
    "Goblin Worker"
}

async fn platform_handler() -> axum::Json<PlatformInfo> {
    axum::Json(PlatformInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        family: std::env::consts::FAMILY.to_string(),
    })
}

async fn health_handler(State(state): State<Arc<state::AppState>>) -> axum::Json<HealthReport> {
    let uptime_seconds = START_TIME.get().map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let scheduled_tasks = state
        .scheduler()
        .map(|s: Arc<scheduler::Scheduler>| s.list_tasks().unwrap_or_default().len())
        .unwrap_or(0);
    axum::Json(HealthReport {
        worker_id: state.worker_id.to_string(),
        status: WorkerStatus::Online,
        paired: state.is_mtls_active() || state.pairing_hash.lock().is_some(),
        uptime_seconds,
        load: 0,
        active_traces: state.traces.lock().len(),
        scheduled_tasks,
        version: env!("CARGO_PKG_VERSION"),
    })
}

mod daemonize {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;
    use std::process::Command;

    pub fn daemonize(pid_file: &Path, log_file: &Path) -> anyhow::Result<()> {
        let pid = std::process::id();
        std::fs::create_dir_all(pid_file.parent().unwrap_or(Path::new("/")))?;
        std::fs::create_dir_all(log_file.parent().unwrap_or(Path::new("/")))?;
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(pid_file)?;
            writeln!(f, "{}", pid)?;
        }

        let current_exe = std::env::current_exe()?;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut cmd = Command::new(current_exe);
        cmd.args(&args).stdout(std::process::Stdio::null()).stderr(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)?,
        );
        // Remove --daemon from args to avoid respawning loop
        let filtered: Vec<String> = args
            .into_iter()
            .filter(|a| {
                a != "--daemon" && !a.starts_with("--pid-file") && !a.starts_with("--log-file")
            })
            .collect();
        cmd.args(filtered);
        let _ = cmd.spawn()?;
        std::process::exit(0);
    }
}
