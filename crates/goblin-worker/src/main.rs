use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use clap::Parser;
use goble_core::tls::PairingBundle;
use goble_core::worker::WorkerId;
use tracing_subscriber::EnvFilter;

mod file_vault;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let worker_id = match args.worker_id {
        Some(id) => WorkerId(id),
        None => WorkerId::generate(),
    };

    let state = state::AppState::new(worker_id);
    {
        let mut config = state.config.lock();
        config.workspace_root = args.workspace_root;
    }

    let task_store = task_store::TaskStore::open(args.task_store)?;
    let scheduler = Arc::new(scheduler::Scheduler::new(state.clone(), task_store));
    let scheduler_for_state = Arc::clone(&scheduler);
    scheduler.start_loop(Duration::from_secs(5));
    state.set_scheduler(scheduler_for_state);

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/pair", post(pairing::pair_handler))
        .route("/status", get(pairing::status_handler))
        .route("/ws", get(websocket::ws_handler))
        .with_state(state);

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
