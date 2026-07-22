use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod pairing;
mod runner;
mod state;
mod websocket;

use goble_core::worker::WorkerId;

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/pair", post(pairing::pair_handler))
        .route("/status", get(pairing::status_handler))
        .route("/ws", get(websocket::ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;
    tracing::info!("goblin listening on {}", args.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn root_handler() -> &'static str {
    "Goblin Worker"
}
