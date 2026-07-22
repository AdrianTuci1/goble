use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures::SinkExt;
use goble_core::agent::{AgentSpec, Trigger};
use goble_core::crypto::{generate_pairing_code, hash_pairing_code};
use goble_core::protocol::DesktopMessage;
use goble_core::store::Store;
use goble_core::worker::{WorkerConfig, WorkerId};

#[derive(Parser, Debug)]
#[command(name = "goble-cli")]
#[command(about = "Goble command-line interface")]
struct Args {
    #[arg(long, default_value = "~/.config/goble/goble.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
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
    /// Generate a pairing code hash for a worker.
    Pair { code: Option<String> },
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
}

fn main() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<()> {
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
        Command::Pair { code } => {
            let code = code.unwrap_or_else(generate_pairing_code);
            let hash = hash_pairing_code(&code, &[0u8; 16])?;
            println!("code: {}\nhash: {}", code, hash);
        }
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
                &worker,
                &url,
                &code,
                DesktopMessage::RunAgent {
                    trace_id,
                    agent_id: agent.id.clone(),
                    spec: agent,
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
                &worker,
                &url,
                &code,
                DesktopMessage::ScheduleAgent {
                    agent_id: goble_core::agent::AgentId(agent_id),
                    trigger,
                },
            )
            .await?;
            println!("schedule sent");
        }
    }

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
    worker_id: &str,
    url: &str,
    pairing_code: &str,
    msg: DesktopMessage,
) -> Result<()> {
    let hash = hash_pairing_code(pairing_code, &[0u8; 16])?;
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(url).await?;
    let pair = DesktopMessage::PairRequest {
        worker_id: WorkerId(worker_id.to_string()),
        pairing_code_hash: hash,
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
