use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

mod state;
mod tui;
mod worker_manager;

use goble_core::store::Store;

#[derive(Parser, Debug)]
#[command(name = "goble")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:0")]
    worker: Option<String>,
    #[arg(long)]
    pairing_code: Option<String>,
    #[arg(long)]
    worker_id: Option<String>,
    #[arg(long)]
    worker_name: Option<String>,
    #[arg(long)]
    run_headless: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let store = Store::open_in_memory()?;
    let state = state::DesktopState::new(store);

    if let Some(worker_url) = args.worker {
        let worker_id = args
            .worker_id
            .map(goble_core::worker::WorkerId)
            .unwrap_or_else(goble_core::worker::WorkerId::generate);
        let name = args.worker_name.unwrap_or_else(|| "default".to_string());
        let code = args.pairing_code.unwrap_or_else(|| "00000000".to_string());
        let mut app = tui::App::new(state.clone());
        app.connect_worker(worker_id.clone(), name, worker_url.clone(), code)
            .await?;
        println!("connected to worker {} at {}", worker_id, worker_url);
    }

    if args.run_headless {
        println!("running headless. press ctrl+c to exit.");
        tokio::signal::ctrl_c().await?;
    } else {
        tui::run_tui(state).await?;
    }

    Ok(())
}
