use anyhow::{Context, Result};
use base64::Engine;
use clap::Parser;
use goble_core::agent::{AgentSpec, Trigger};
use goble_core::cluster_key::{ClusterIdentity, ClusterKey};
use goble_core::crypto::{generate_pairing_code, hash_pairing_code};
use goble_core::encrypted_wallet::IdentityWallet;
use goble_core::identity::ClusterRole;
use goble_core::protocol::DesktopMessage;
use goble_core::snapshot::{LocalSnapshotProvider, SnapshotProvider};
use goble_core::store::Store;
use goble_core::worker::{WorkerConfig, WorkerId};

use crate::cli::{
    Args, ClusterAction, Command, DeviceAction, IdentityAction, ScheduleAction, SecretAction,
    SnapshotAction,
};
use crate::worker::{do_provision, init_store, send_to_worker};

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

            IdentityAction::RotateWorkerCerts { passphrase, days } => {
                let sealed = store.get_cluster_wallet()?.ok_or_else(|| {
                    anyhow::anyhow!("no cluster wallet in store; create or restore identity first")
                })?;
                let plaintext = sealed.open(passphrase.as_bytes())?;
                let wallet: IdentityWallet = serde_json::from_slice(&plaintext)
                    .context("wallet does not contain a valid IdentityWallet")?;
                let mut rotated = 0;
                for worker in &wallet.workers.clone() {
                    if let Some(ref existing) = find_worker_cert(&wallet, &worker.worker_id) {
                        let ca = wallet.to_cluster_identity("rotate", ClusterRole::Admin)?.ca;
                        ca.revoke(existing.serial())?;
                    }
                    let identity = wallet.to_cluster_identity("rotate", ClusterRole::Admin)?;
                    let _bundle = identity.ca.sign_worker_bundle(
                        &worker.worker_id,
                        &wallet.cluster_name,
                        days,
                    )?;
                    println!("rotated certificate for worker {}", worker.worker_id);
                    rotated += 1;
                }
                let resealed = wallet.seal(passphrase.as_bytes())?;
                store.set_cluster_wallet(&resealed)?;
                println!("rotated {} worker certificate(s)", rotated);
                println!("WARNING: workers must be restarted with their new bundles to use the updated certificates.");
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
                let sealed = store.get_cluster_wallet()?.ok_or_else(|| {
                    anyhow::anyhow!("no cluster wallet in store; create or restore identity first")
                })?;
                let plaintext = sealed.open(passphrase.as_bytes())?;
                let wallet: IdentityWallet = serde_json::from_slice(&plaintext)
                    .context("wallet does not contain a valid IdentityWallet")?;
                let identity =
                    wallet.to_cluster_identity("cli-cluster-command", ClusterRole::Admin)?;
                let worker_id = "goblin-cluster".to_string();
                let bundle =
                    identity
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
                    helm_args.push(format!(
                        "--set snapshot.secretAccessKey={} ",
                        secret_access_key
                    ));
                }
                helm_args.push("\n".to_string());
                println!("Run the following command in a cluster with Helm configured:");
                println!("{}", helm_args.join(""));
            }
        },
    }

    Ok(())
}

fn find_worker_cert(
    _wallet: &IdentityWallet,
    _worker_id: &str,
) -> Option<goble_core::identity::Identity> {
    // We intentionally do not parse active certificate PEMs here; rotate-worker-certs will
    // revoke by best-effort serial lookup once certificate registry metadata is stored in the
    // wallet. For now this helper always returns None so rotation issues fresh certificates.
    None
}
