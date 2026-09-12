use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use futures::SinkExt;
use goble_core::crypto::{generate_pairing_code, hash_pairing_code};
use goble_core::protocol::DesktopMessage;
use goble_core::provision::{provision_worker, LocalTransport, ProvisionConfig, SshTransport};
use goble_core::store::Store;
use goble_core::tls::CertGenerator;
use goble_core::worker::{WorkerConfig, WorkerId};

#[allow(clippy::too_many_arguments)]
pub(crate) fn do_provision(
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

pub(crate) async fn init_store() -> Result<Store> {
    let path = dirs::config_dir()
        .map(|p| p.join("goble"))
        .unwrap_or_else(|| PathBuf::from(".goble"));
    std::fs::create_dir_all(&path)?;
    Store::open(path.join("store.db"))
}

pub(crate) async fn send_to_worker(
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
