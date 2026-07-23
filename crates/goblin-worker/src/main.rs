use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::routing::{get, post};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use goble_core::tls::PairingBundle;
use goble_core::worker::{WorkerId, WorkerStatus};
use serde::Serialize;
use tracing_subscriber::EnvFilter;

mod file_vault;
mod mcp;
mod pairing;
mod runner;
mod scheduler;
mod state;
mod task_store;
mod websocket;

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

    let state = state::AppState::new(worker_id.clone());
    {
        let mut config = state.config.lock();
        config.workspace_root = args.workspace_root;
    }

    state.set_vault_path(args.vault_path);

    let task_store = task_store::TaskStore::open(args.task_store)?;
    let scheduler = Arc::new(scheduler::Scheduler::new(state.clone(), task_store));
    let scheduler_for_state = Arc::clone(&scheduler);
    scheduler.start_loop(Duration::from_secs(5));
    state.set_scheduler(scheduler_for_state);

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/health", get(health_handler))
        .route("/mcp", get(mcp::list_mcp_handler))
        .route("/pair", post(pairing::pair_handler))
        .route("/status", get(pairing::status_handler))
        .route("/ws", get(websocket::ws_handler))
        .with_state(state.clone());

    if let Some(bundle_path) = args.tls_bundle {
        let bundle_json = tokio::fs::read_to_string(&bundle_path).await?;
        let bundle: PairingBundle = serde_json::from_str(&bundle_json)?;
        let rustls_config = RustlsConfig::from_config(Arc::new(bundle.server_config()?));
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

async fn health_handler(State(state): State<Arc<state::AppState>>) -> axum::Json<HealthReport> {
    let uptime_seconds = START_TIME.get().map(|t| t.elapsed().as_secs()).unwrap_or(0);
    let scheduled_tasks = state
        .scheduler()
        .map(|s: Arc<scheduler::Scheduler>| s.list_tasks().unwrap_or_default().len())
        .unwrap_or(0);
    axum::Json(HealthReport {
        worker_id: state.worker_id.to_string(),
        status: WorkerStatus::Online,
        paired: state.pairing_hash.lock().is_some(),
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
    use std::os::unix::process::CommandExt;
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
