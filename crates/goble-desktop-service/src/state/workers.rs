use std::sync::Arc;

use anyhow::Context;
use chrono::Utc;
use goble_core::protocol::DesktopMessage;
use goble_core::worker::{WorkerConfig, WorkerId};
use goble_core::worker_pool::{WorkerPool, WorkerPoolStrategy, WorkerSnapshot};

use crate::worker_manager::WorkerClient;

use super::{DesktopState, WorkerConnection};

const WORKER_PAIRING_CODE_VAULT_PREFIX: &str = "worker:";

impl DesktopState {
    /// Reconnect workers that were previously paired and have a stored pairing code in the vault.
    pub fn restore_clients(self: Arc<Self>) {
        let paired: Vec<(WorkerId, String)> = self
            .workers
            .lock()
            .values()
            .filter(|c| c.paired)
            .map(|c| (WorkerId(c.id.clone()), c.url.clone()))
            .collect();
        if paired.is_empty() {
            return;
        }
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            self.add_log("vault locked; skip auto-reconnect for paired workers");
            return;
        }
        for (wid, _) in paired {
            let config = match self.get_worker_config(&wid) {
                Ok(c) => c,
                Err(e) => {
                    self.add_log(format!("no config for worker {wid}; skip reconnect: {e}"));
                    continue;
                }
            };
            let vault_key = format!("{WORKER_PAIRING_CODE_VAULT_PREFIX}{}:pairing_code", wid);
            let code = match self.vault.lock().get(&vault_key, &passphrase) {
                Ok(Some(v)) => String::from_utf8_lossy(&v).to_string(),
                Ok(None) => {
                    self.add_log(format!(
                        "no stored pairing code for worker {wid}; skip reconnect"
                    ));
                    continue;
                }
                Err(e) => {
                    self.add_log(format!(
                        "failed to decrypt pairing code for worker {wid}: {e}"
                    ));
                    continue;
                }
            };
            let state = self.clone();
            let worker_id = wid.clone();
            tokio::spawn(async move {
                match WorkerClient::connect(state.clone(), worker_id.clone(), &config, code).await {
                    Ok(client) => {
                        state.clients.lock().insert(worker_id.clone(), client);
                        state.add_log(format!("worker {worker_id} reconnected"));
                    }
                    Err(e) => {
                        state.add_log(format!("failed to reconnect worker {worker_id}: {e}"));
                    }
                }
            });
        }
    }

    fn store_pairing_code(&self, worker_id: &WorkerId, pairing_code: &str) {
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            return;
        }
        let vault_key = format!(
            "{WORKER_PAIRING_CODE_VAULT_PREFIX}{}:pairing_code",
            worker_id
        );
        let _ = self
            .vault
            .lock()
            .set(&vault_key, pairing_code.as_bytes(), &passphrase);
        if let Ok(bytes) = self.vault.lock().to_bytes() {
            let _ = self
                .store
                .lock()
                .set_setting("vault_blob", &String::from_utf8_lossy(&bytes));
        }
    }

    pub(super) fn device_id() -> String {
        format!(
            "desktop-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("unknown")
        )
    }

    pub fn add_worker(&self, worker_id: WorkerId, name: String, url: String) -> anyhow::Result<()> {
        let conn = WorkerConnection {
            id: worker_id.to_string(),
            name: name.clone(),
            url: url.clone(),
            paired: false,
            tags: Vec::new(),
        };
        let config = WorkerConfig::new(&name, &url, "");
        self.store.lock().insert_worker(
            &worker_id.to_string(),
            &conn.name,
            Some(&url),
            "unpaired",
            None,
            &serde_json::to_string(&config)?,
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
        )?;
        self.workers.lock().insert(worker_id, conn);
        self.emit("workers:updated", ());
        Ok(())
    }

    pub fn tag_worker(&self, worker_id: &WorkerId, tag: String) -> anyhow::Result<()> {
        let mut workers = self.workers.lock();
        let conn = workers
            .get_mut(worker_id)
            .ok_or_else(|| anyhow::anyhow!("worker not found"))?;
        if !conn.tags.contains(&tag) {
            conn.tags.push(tag.clone());
        }
        let mut config: WorkerConfig = serde_json::from_str(
            &self
                .store
                .lock()
                .get_worker(&worker_id.to_string())?
                .map(|(_, _, _, cfg)| cfg)
                .unwrap_or_default(),
        )
        .unwrap_or_else(|_| WorkerConfig::new(&conn.name, &conn.url, ""));
        config.tags = conn.tags.clone();
        self.store.lock().insert_worker(
            &worker_id.to_string(),
            &conn.name,
            Some(&conn.url),
            if conn.paired { "paired" } else { "unpaired" },
            None,
            &serde_json::to_string(&config)?,
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
        )?;
        self.emit("workers:updated", ());
        Ok(())
    }

    pub fn remove_worker(&self, worker_id: &WorkerId) {
        self.workers.lock().remove(worker_id);
        let _ = self.store.lock().delete_worker(&worker_id.to_string());
        self.clients.lock().remove(worker_id);
        self.emit("workers:updated", ());
    }

    pub fn list_workers(&self) -> Vec<WorkerConnection> {
        self.workers.lock().values().cloned().collect()
    }

    pub fn get_worker_config(&self, worker_id: &WorkerId) -> anyhow::Result<WorkerConfig> {
        match self.store.lock().get_worker(&worker_id.to_string())? {
            Some((_, _, _, config_json)) => {
                serde_json::from_str(&config_json).context("invalid worker config")
            }
            None => anyhow::bail!("worker not found"),
        }
    }

    pub fn pair_worker(
        self: Arc<Self>,
        worker_id: &WorkerId,
        pairing_code: String,
    ) -> anyhow::Result<bool> {
        let conn = self.workers.lock().get(worker_id).cloned();
        let cluster = self.get_cluster_identity();
        if let Some(conn) = conn {
            let state = self.clone();
            let wid = worker_id.clone();
            let code = pairing_code.clone();
            tokio::spawn(async move {
                let conn_name = conn.name.clone();
                let url = conn.url.clone();
                let config = match cluster {
                    Some(identity) => {
                        let bundle = match identity.ca.sign_worker_bundle(
                            &wid.to_string(),
                            &identity.cluster_name,
                            365,
                        ) {
                            Ok(b) => b,
                            Err(e) => {
                                state.add_log(format!(
                                    "failed to sign worker bundle for {}: {}",
                                    wid, e
                                ));
                                return;
                            }
                        };
                        let mut cfg = WorkerConfig::new(&conn_name, &url, "");
                        cfg.id = wid.clone();
                        cfg.port = url
                            .split(':')
                            .next_back()
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(7878);
                        cfg.worker_bundle = Some(bundle);
                        cfg.desktop_identity = Some(identity.device);
                        cfg
                    }
                    None => {
                        let mut cfg = WorkerConfig::new(&conn_name, &url, "");
                        cfg.id = wid.clone();
                        cfg.port = url
                            .split(':')
                            .next_back()
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(7878);
                        cfg
                    }
                };
                match WorkerClient::connect(state.clone(), wid.clone(), &config, code.clone()).await
                {
                    Ok(client) => {
                        state.clients.lock().insert(wid.clone(), client);
                        if let Some(c) = state.workers.lock().get_mut(&wid) {
                            c.paired = true;
                        }
                        let config_json =
                            serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());
                        let _ = state.store.lock().insert_worker(
                            &wid.to_string(),
                            &conn_name,
                            Some(&config.websocket_url()),
                            "paired",
                            None,
                            &config_json,
                            &Utc::now().to_rfc3339(),
                            &Utc::now().to_rfc3339(),
                        );
                        state.store_pairing_code(&wid, &code);
                        state.add_log(format!("worker {} paired", wid));
                        state.emit("workers:updated", ());
                    }
                    Err(e) => {
                        state.add_log(format!("failed to connect worker {}: {}", wid, e));
                    }
                }
            });
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn send_to_worker(&self, worker_id: &WorkerId, msg: DesktopMessage) -> anyhow::Result<()> {
        if let Some(client) = self.clients.lock().get(worker_id) {
            let _ = client.send(msg);
            Ok(())
        } else {
            anyhow::bail!("worker not connected: {}", worker_id)
        }
    }

    /// Resolve an abstract runtime target to a concrete paired worker id.
    pub fn resolve_worker_for_target(
        &self,
        target_kind: &str,
        tag: Option<&str>,
        worker_id: Option<&str>,
    ) -> anyhow::Result<WorkerId> {
        let all_workers = self.list_workers();
        let paired_workers: Vec<WorkerConnection> =
            all_workers.into_iter().filter(|w| w.paired).collect();

        if target_kind == "worker" {
            let id = worker_id.ok_or_else(|| anyhow::anyhow!("worker target missing worker_id"))?;
            if paired_workers.iter().any(|w| w.id == id) {
                return Ok(WorkerId(id.to_string()));
            }
            anyhow::bail!("worker {} is not paired", id);
        }

        if target_kind == "local" {
            anyhow::bail!("local runtime target is not supported yet");
        }

        let strategy = match tag {
            Some(t) => WorkerPoolStrategy::TaggedFirst { tag: t.to_string() },
            None => WorkerPoolStrategy::RoundRobin,
        };
        let mut pool = WorkerPool::new(strategy);
        let snapshots: Vec<WorkerSnapshot> = paired_workers
            .iter()
            .map(|w| WorkerSnapshot {
                worker_id: WorkerId(w.id.clone()),
                name: w.name.clone(),
                url: w.url.clone(),
                status: goble_core::worker::WorkerStatus::Online,
                load: 0,
                tags: w.tags.clone(),
            })
            .collect();
        pool.select(&snapshots)
            .map(|s| s.worker_id.clone())
            .ok_or_else(|| anyhow::anyhow!("no paired worker available for target {}", target_kind))
    }
}
