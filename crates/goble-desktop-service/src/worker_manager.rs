use std::io::Write as StdWrite;
use std::sync::Arc;

use anyhow::Context;
use futures::{SinkExt, StreamExt};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::{WorkerConfig, WorkerId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, connect_async_tls_with_config, MaybeTlsStream, WebSocketStream};

use crate::state::DesktopState;

pub struct WorkerClient {
    pub worker_id: WorkerId,
    pub url: String,
    tx: mpsc::UnboundedSender<DesktopMessage>,
    /// Holds a temporary SSH private key file for SSH connections. The file is
    /// deleted when the client is dropped, which is fine because the SSH process
    /// only needs it during initial authentication.
    #[allow(dead_code)]
    _key_file: Option<tempfile::NamedTempFile>,
    /// Holds the spawned SSH child process so it is not killed while the
    /// connection is alive. The stdin/stdout/stderr handles have been taken
    /// by the reader/writer tasks; keeping the `Child` prevents `kill_on_drop`
    /// from terminating the remote worker.
    #[allow(dead_code)]
    _child: Option<tokio::process::Child>,
}

impl WorkerClient {
    pub async fn connect(
        state: Arc<DesktopState>,
        worker_id: WorkerId,
        config: &WorkerConfig,
        pairing_code: String,
    ) -> anyhow::Result<Self> {
        let url = config.websocket_url();
        let ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>> =
            if let Some(ref bundle) = config.worker_bundle {
                let client_config =
                    bundle.client_config(config.desktop_identity.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("worker bundle present but no desktop identity configured")
                    })?)?;
                let connector = tokio_tungstenite::Connector::Rustls(Arc::new(client_config));
                let (ws_stream, _) =
                    connect_async_tls_with_config(&url, None, true, Some(connector)).await?;
                ws_stream
            } else {
                let (ws_stream, _) = connect_async(&url).await?;
                ws_stream
            };
        let (mut write, mut read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<DesktopMessage>();

        let hash = goble_core::crypto::hash_pairing_code(&pairing_code, &[0u8; 16])?;

        let pair_msg = DesktopMessage::PairRequest {
            worker_id: worker_id.clone(),
            pairing_code_hash: Some(hash),
        };
        let json = serde_json::to_string(&pair_msg)?;
        write
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| anyhow::anyhow!("send pair: {}", e))?;

        let worker_id_clone = worker_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if write.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            let _ = write.close().await;
        });

        let state_clone = state.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    if let Ok(worker_msg) = serde_json::from_str::<WorkerMessage>(&text) {
                        state_clone.handle_worker_message(&worker_id_clone, worker_msg);
                    }
                }
            }
            state_clone.remove_worker(&worker_id_clone);
        });

        Ok(Self {
            worker_id,
            url,
            tx,
            _key_file: None,
            _child: None,
        })
    }

    /// Connect to a worker over SSH by spawning `goblin --ssh-proxy` on the remote
    /// host. The remote machine only needs SSH (port 22) exposed; no worker TCP
    /// port is opened.
    #[cfg(unix)]
    pub async fn connect_ssh(
        state: Arc<DesktopState>,
        worker_id: WorkerId,
        creds: &crate::ssh_installer::SshCredentials,
        remote_binary: &std::path::Path,
        pairing_code: String,
    ) -> anyhow::Result<Self> {
        use std::os::unix::fs::PermissionsExt;

        // Write the private key to a temporary file with restrictive permissions.
        // SSH requires a file, and keeping it inside WorkerClient ensures it lives
        // as long as the connection (the SSH process reads it during handshake).
        let mut key_file = tempfile::NamedTempFile::new()?;
        key_file.write_all(creds.private_key.as_bytes())?;
        let key_path = key_file.path().to_path_buf();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;

        let remote_root: std::path::PathBuf = std::env::var("GOBLIN_SSH_REMOTE_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/goblin"));
        let workspace_root = remote_root.join("workspaces");
        let task_store = remote_root.join("tasks.db");
        let vault_path = remote_root.join("vault.json");
        let pvc_root = remote_root.clone();

        let remote_cmd = format!(
            "{} --ssh-proxy --workspace-root {} --task-store {} --vault-path {} --pvc-root {} --worker-id {}",
            remote_binary.display(),
            workspace_root.display(),
            task_store.display(),
            vault_path.display(),
            pvc_root.display(),
            worker_id.0
        );

        let url = format!("ssh://{}@{}:{}", creds.user, creds.host, creds.port);
        let mut cmd = Command::new("ssh");
        cmd.arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=10")
            .arg("-p")
            .arg(creds.port.to_string())
            .arg("-i")
            .arg(&key_path)
            .arg(format!("{}@{}", creds.user, creds.host))
            .arg(&remote_cmd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        Self::spawn_command(state, worker_id, url, cmd, Some(key_file), pairing_code).await
    }

    /// Shared protocol loop for any spawned command that speaks NDJSON
    /// DesktopMessage/WorkerMessage on stdin/stdout (e.g. `goblin --ssh-proxy` or
    /// the local goblin binary in tests).
    async fn spawn_command(
        state: Arc<DesktopState>,
        worker_id: WorkerId,
        url: String,
        mut cmd: Command,
        key_file: Option<tempfile::NamedTempFile>,
        pairing_code: String,
    ) -> anyhow::Result<Self> {
        let mut child = cmd.spawn().context("spawn worker command")?;
        let stdin = child
            .stdin
            .take()
            .context("worker command missing stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("worker command missing stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("worker command missing stderr")?;

        let (tx, mut rx) = mpsc::unbounded_channel::<DesktopMessage>();

        // Send the pairing request as the first message.
        let hash = goble_core::crypto::hash_pairing_code(&pairing_code, &[0u8; 16])?;
        let pair_msg = DesktopMessage::PairRequest {
            worker_id: worker_id.clone(),
            pairing_code_hash: Some(hash),
        };
        let pair_json = serde_json::to_string(&pair_msg)?;
        let mut writer = tokio::io::BufWriter::new(stdin);
        writer.write_all(pair_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Writer task: forward DesktopMessages as NDJSON to the command's stdin.
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if writer.write_all(json.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        // Reader task: parse NDJSON WorkerMessages and dispatch them.
        let state_clone = state.clone();
        let worker_id_reader = worker_id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(worker_msg) = serde_json::from_str::<WorkerMessage>(&line) {
                    state_clone.handle_worker_message(&worker_id_reader, worker_msg);
                }
            }
            state_clone.remove_worker(&worker_id_reader);
        });

        // Stderr logger task.
        let state_log = state.clone();
        let worker_id_log = worker_id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                state_log.add_log(format!("[{}] {}", worker_id_log, line));
            }
        });

        Ok(Self {
            worker_id,
            url,
            tx,
            _key_file: key_file,
            _child: Some(child),
        })
    }

    pub fn send(&self, msg: DesktopMessage) -> anyhow::Result<()> {
        self.tx
            .send(msg)
            .map_err(|e| anyhow::anyhow!("channel closed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::store::Store;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_worker_client_connect_mock() {
        use tokio_tungstenite::accept_async;

        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        let worker_id = WorkerId::generate();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("ws://127.0.0.1:{}/ws", port);
        state
            .add_worker(worker_id.clone(), "mock".to_string(), url.clone())
            .unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(text) = msg {
                    if let Ok(desktop_msg) = serde_json::from_str::<DesktopMessage>(&text) {
                        match desktop_msg {
                            DesktopMessage::PairRequest {
                                worker_id,
                                pairing_code_hash: _,
                            } => {
                                let resp = WorkerMessage::Paired;
                                let _ = ws
                                    .send(Message::Text(
                                        serde_json::to_string(&resp).unwrap().into(),
                                    ))
                                    .await;
                                assert_eq!(worker_id, WorkerId(worker_id.0.clone()));
                            }
                            DesktopMessage::Ping => {
                                let resp = WorkerMessage::Pong;
                                let _ = ws
                                    .send(Message::Text(
                                        serde_json::to_string(&resp).unwrap().into(),
                                    ))
                                    .await;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        let client = WorkerClient::connect(
            state.clone(),
            worker_id.clone(),
            &WorkerConfig::new("mock", "127.0.0.1", "")
                .with_pairing_code("0000")
                .with_worker_id(worker_id.clone())
                .with_port(port),
            "0000".to_string(),
        )
        .await;
        assert!(client.is_ok(), "{:?}", client.err());
        let client = client.unwrap();
        client.send(DesktopMessage::Ping).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(state.list_workers()[0].paired);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[cfg(unix)]
    async fn test_worker_client_connect_ssh_spawns_and_pairs() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );

        // Build a fake `ssh` executable that simply execs the remote command
        // (the last argument passed by WorkerClient::connect_ssh).
        let temp_dir = tempfile::tempdir().unwrap();
        let ssh_dir = temp_dir.path().join("ssh");
        std::fs::create_dir(&ssh_dir).unwrap();
        let fake_ssh = ssh_dir.join("ssh");
        {
            let mut f = std::fs::File::create(&fake_ssh).unwrap();
            f.write_all(
                b"#!/bin/bash\nlast=\"\"\nfor a in \"$@\"; do last=\"$a\"; done\nexec /bin/bash -c \"$last\"\n",
            )
            .unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(&ssh_dir);
        new_path.push(":");
        new_path.push(&old_path);
        std::env::set_var("PATH", new_path);

        let remote_root = temp_dir.path().join("goblin_remote");
        std::fs::create_dir_all(remote_root.join("workspaces")).unwrap();
        std::env::set_var("GOBLIN_SSH_REMOTE_ROOT", &remote_root);

        let worker_id = WorkerId::generate();
        let remote_binary = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target/debug/goblin");

        // Make sure the goblin binary exists for the fake ssh script to spawn.
        if !remote_binary.exists() {
            let build = tokio::process::Command::new("cargo")
                .args(["build", "-p", "goblin-worker", "--bin", "goblin"])
                .output()
                .await
                .unwrap();
            assert!(
                build.status.success(),
                "failed to build goblin binary:\n{}",
                String::from_utf8_lossy(&build.stderr)
            );
        }
        assert!(
            remote_binary.exists(),
            "goblin binary not found at {}",
            remote_binary.display()
        );

        let url = format!("ssh://test@localhost:22");
        state
            .add_worker(worker_id.clone(), "ssh-local".to_string(), url)
            .unwrap();

        let creds = crate::ssh_installer::SshCredentials {
            host: "localhost".to_string(),
            user: "test".to_string(),
            port: 22,
            private_key: "not-a-real-key".to_string(),
        };
        let client = WorkerClient::connect_ssh(
            state.clone(),
            worker_id.clone(),
            &creds,
            &remote_binary,
            "0000".to_string(),
        )
        .await;

        assert!(client.is_ok(), "{}", client.err().map(|e| e.to_string()).unwrap_or_default());

        let client = client.unwrap();
        client.send(DesktopMessage::Ping).unwrap();
        tokio::time::sleep(Duration::from_millis(600)).await;

        let workers = state.list_workers();
        assert_eq!(workers.len(), 1);
        assert!(
            workers[0].paired,
            "ssh-proxy worker should be marked paired after it emits Paired"
        );
        assert!(
            state.get_logs().iter().any(|l| l.message.contains("pong")),
            "ssh-proxy worker should respond to Ping with Pong"
        );

        std::env::set_var("PATH", old_path);
        std::env::remove_var("GOBLIN_SSH_REMOTE_ROOT");
    }
}
