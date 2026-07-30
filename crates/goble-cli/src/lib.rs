use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures::SinkExt;
use goble_core::agent::{AgentSpec, Trigger};
use goble_core::crypto::{generate_pairing_code, hash_pairing_code};
use goble_core::protocol::DesktopMessage;
use goble_core::store::Store;
use goble_core::tls::{CertGenerator, PairingBundle};
use goble_core::provision::{self, LocalTransport, SshTransport, ProvisionConfig, provision_worker};
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
                    &worker,
                    &url,
                    &code,
                    None,
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
                    &worker,
                    &url,
                    &code,
                    None,
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
                &worker,
                &url,
                &code,
                None,
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
                &worker,
                &url,
                &code,
                None,
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
                    &worker,
                    &url,
                    &code,
                    None,
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
                    &worker,
                    &url,
                    &code,
                    None,
                    DesktopMessage::GetVaultSecret { name },
                )
                .await?;
                println!("secret get request sent");
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
    let tls_bundle = PairingBundle {
        ca_cert_pem: ca.cert_pem,
        ca_key_pem: None,
        worker_cert_pem: server.cert_pem,
        worker_key_pem: server.key_pem,
        desktop_cert_pem: desktop.cert_pem,
        desktop_key_pem: desktop.key_pem,
        pairing_code_hash: pairing_hash.clone(),
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
        tls_bundle,
    };

    if local_test {
        let tmp = tempfile::TempDir::new()?;
        let transport = LocalTransport::new(tmp.path());
        provision_worker(&transport, &config)?;
    } else {
        let transport = SshTransport::new(&host, &username, ssh_key);
        provision_worker(&transport, &config)?;
    }

    store.insert_worker(
        &worker_id.0,
        &name,
        Some(&format!("{}:8787", host)),
        "provisioned",
        None,
        &serde_json::to_string(
            &WorkerConfig::new(&name, &host, &username).with_pairing_code(&pairing_code),
        )?,
        "",
        "",
    )?;

    println!(
        "provisioned worker {} ({}) on {} with pairing code {}",
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
    worker_id: &str,
    url: &str,
    pairing_code: &str,
    bundle: Option<&PairingBundle>,
    msg: DesktopMessage,
) -> Result<()> {
    let hash = hash_pairing_code(pairing_code, &[0u8; 16])?;
    let mut ws_stream = connect_async_with_tls(url, bundle).await?;
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

async fn connect_async_with_tls(
    url: &str,
    bundle: Option<&PairingBundle>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    if let Some(bundle) = bundle {
        let tls_config = bundle.client_config()?;
        let connector = tokio_tungstenite::Connector::Rustls(Arc::new(tls_config));
        let (stream, resp) =
            tokio_tungstenite::connect_async_tls_with_config(url, None, false, Some(connector))
                .await?;
        let _ = resp;
        Ok(stream)
    } else {
        let (stream, _) = tokio_tungstenite::connect_async(url).await?;
        Ok(stream)
    }
}
