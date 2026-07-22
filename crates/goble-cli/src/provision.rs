use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result};
use goble_core::tls::PairingBundle;

/// Transport used to copy files and run commands on a remote host.
pub trait ProvisionTransport: Send + Sync {
    /// Copy a local file to a remote path.
    fn copy_file(&self, local: &Path, remote: &str) -> Result<()>;
    /// Run a shell command on the remote host and return stdout.
    fn run_command(&self, command: &str) -> Result<String>;
}

/// SSH-based provisioning transport.
pub struct SshTransport {
    pub host: String,
    pub username: String,
    pub ssh_key: Option<PathBuf>,
}

impl SshTransport {
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        ssh_key: Option<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            username: username.into(),
            ssh_key,
        }
    }

    fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
        ];
        if let Some(key) = &self.ssh_key {
            args.push("-i".to_string());
            args.push(key.display().to_string());
        }
        args.push(format!("{}@{}", self.username, self.host));
        args
    }
}

impl ProvisionTransport for SshTransport {
    fn copy_file(&self, local: &Path, remote: &str) -> Result<()> {
        let mut cmd = std::process::Command::new("scp");
        cmd.arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=10");
        if let Some(key) = &self.ssh_key {
            cmd.arg("-i").arg(key);
        }
        cmd.arg(local)
            .arg(format!("{}@{}:{}", self.username, self.host, remote));
        let output = cmd.output().context("scp command failed to start")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("scp failed: {}", stderr);
        }
        Ok(())
    }

    fn run_command(&self, command: &str) -> Result<String> {
        let mut ssh = std::process::Command::new("ssh");
        ssh.args(self.ssh_args())
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = ssh.output().context("ssh command failed to start")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ssh command failed: {}", stderr);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Local transport for testing provisioning on the same machine.
pub struct LocalTransport {
    pub root: PathBuf,
}

impl LocalTransport {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl ProvisionTransport for LocalTransport {
    fn copy_file(&self, local: &Path, remote: &str) -> Result<()> {
        let dest = self.root.join(remote.trim_start_matches('/'));
        std::fs::create_dir_all(dest.parent().unwrap_or(&self.root))?;
        std::fs::copy(local, &dest).with_context(|| format!("failed to copy to {:?}", dest))?;
        Ok(())
    }

    fn run_command(&self, command: &str) -> Result<String> {
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .env(
                "GOBLE_LOCAL_PROVISION_ROOT",
                self.root.display().to_string(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("local bash command failed")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("local command failed: {}", stderr);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Configuration for a worker installation.
#[derive(Debug, Clone)]
pub struct ProvisionConfig {
    pub worker_id: String,
    #[allow(dead_code)]
    pub name: String,
    pub install_path: String,
    pub workspace_root: String,
    pub pairing_code_hash: String,
    pub install_docker: bool,
    pub install_hermes: bool,
    pub install_crewai: bool,
    pub goblin_binary: PathBuf,
    pub tls_bundle: PairingBundle,
}

/// Generates the shell script that installs the worker on the target host.
pub fn generate_install_script(config: &ProvisionConfig) -> String {
    let mut checks = Vec::new();
    checks.push(
        "command -v curl >/dev/null 2>&1 || (apt-get update && apt-get install -y curl)"
            .to_string(),
    );
    if config.install_docker {
        checks.push(r#"
if ! command -v docker >/dev/null 2>&1; then
  echo "Installing Docker..."
  apt-get update
  apt-get install -y ca-certificates gnupg lsb-release
  mkdir -p /etc/apt/keyrings
  curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
  echo "deb [arch=\"$(dpkg --print-architecture)\" signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" > /etc/apt/sources.list.d/docker.list
  apt-get update
  apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
fi
"#.to_string());
    }
    checks.push("command -v python3 >/dev/null 2>&1 || (apt-get update && apt-get install -y python3 python3-pip python3-venv)".to_string());
    if config.install_hermes {
        checks.push(
            r#"
echo "Hermes runtime install stub: add actual install command here"
"#
            .to_string(),
        );
    }
    if config.install_crewai {
        checks.push(
            r#"
python3 -m pip install --upgrade pip 2>/dev/null || true
python3 -m pip install crewai 2>/dev/null || true
"#
            .to_string(),
        );
    }

    let checks_joined = checks.join("\n");
    let bundle_json = serde_json::to_string(&config.tls_bundle).unwrap_or_default();

    format!(
        r#"#!/bin/bash
set -euo pipefail

INSTALL_PATH={install_path}
WORKSPACE_ROOT={workspace_root}
WORKER_ID={worker_id}
PAIRING_HASH={pairing_hash}
TLS_DIR="$INSTALL_PATH/tls"
BUNDLE_FILE="$TLS_DIR/pairing-bundle.json"

{checks}

mkdir -p "$INSTALL_PATH" "$WORKSPACE_ROOT" "$TLS_DIR"
groupadd -f goblin
id -u goblin >/dev/null 2>&1 || useradd -m -g goblin -s /bin/bash goblin
chown -R goblin:goblin "$INSTALL_PATH" "$WORKSPACE_ROOT"

cat > "$BUNDLE_FILE" <<'EOF'
{bundle_json}
EOF
chmod 600 "$BUNDLE_FILE"
chown goblin:goblin "$BUNDLE_FILE"

mv "$INSTALL_PATH/goblin.new" "$INSTALL_PATH/goblin"
chmod +x "$INSTALL_PATH/goblin"

cat > "$INSTALL_PATH/goblin.env" <<EOF
GOBLIN_WORKER_ID=$WORKER_ID
GOBLIN_WORKSPACE_ROOT=$WORKSPACE_ROOT
GOBLIN_PAIRING_HASH=$PAIRING_HASH
GOBLIN_TLS_BUNDLE=$BUNDLE_FILE
EOF

cat > /etc/systemd/system/goblin.service <<EOF
[Unit]
Description=Goblin Worker
After=network.target

[Service]
Type=simple
User=goblin
Group=goblin
WorkingDirectory=$INSTALL_PATH
EnvironmentFile=$INSTALL_PATH/goblin.env
ExecStart=$INSTALL_PATH/goblin --bind 0.0.0.0:8787 --workspace-root $WORKSPACE_ROOT --worker-id $WORKER_ID --tls-bundle $BUNDLE_FILE
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable goblin.service
systemctl restart goblin.service || echo "goblin service start requested; verify with systemctl status goblin"

echo "Goblin worker $WORKER_ID provisioned at $INSTALL_PATH"
"#,
        install_path = config.install_path,
        workspace_root = config.workspace_root,
        worker_id = config.worker_id,
        pairing_hash = config.pairing_code_hash,
        bundle_json = bundle_json,
        checks = checks_joined,
    )
}

/// Provisions a worker on the target host using the given transport.
pub fn provision_worker(
    transport: &dyn ProvisionTransport,
    config: &ProvisionConfig,
) -> Result<()> {
    let script = generate_install_script(config);
    let script_path = PathBuf::from("/tmp/goblin-install.sh");
    std::fs::write(&script_path, script).context("failed to write install script")?;

    let bundle_path = PathBuf::from("/tmp/goblin-pairing-bundle.json");
    std::fs::write(&bundle_path, serde_json::to_string(&config.tls_bundle)?)
        .context("failed to write pairing bundle")?;

    transport.copy_file(
        &config.goblin_binary,
        &format!("{}/goblin.new", config.install_path),
    )?;
    transport.copy_file(
        &bundle_path,
        &format!("{}/tls/pairing-bundle.json", config.install_path),
    )?;
    transport.copy_file(&script_path, "/tmp/goblin-install.sh")?;
    transport.run_command("chmod +x /tmp/goblin-install.sh && sudo bash /tmp/goblin-install.sh")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_install_script_contains_worker_id() {
        let bundle = PairingBundle {
            ca_cert_pem: "CA\n".to_string(),
            worker_cert_pem: "WORKER\n".to_string(),
            worker_key_pem: "WORKER_KEY\n".to_string(),
            desktop_cert_pem: "DESKTOP\n".to_string(),
            desktop_key_pem: "DESKTOP_KEY\n".to_string(),
            pairing_code_hash: "deadbeef".to_string(),
        };
        let config = ProvisionConfig {
            worker_id: "worker-123".to_string(),
            name: "vps-1".to_string(),
            install_path: "/opt/goblin".to_string(),
            workspace_root: "/var/goblin/workspaces".to_string(),
            pairing_code_hash: "deadbeef".to_string(),
            install_docker: true,
            install_hermes: false,
            install_crewai: true,
            goblin_binary: PathBuf::from("/tmp/goblin"),
            tls_bundle: bundle,
        };
        let script = generate_install_script(&config);
        assert!(script.contains("worker-123"));
        assert!(script.contains("deadbeef"));
        assert!(script.contains("docker-ce"));
        assert!(script.contains("crewai"));
        assert!(script.contains("pairing-bundle.json"));
    }

    #[test]
    fn test_local_transport_copy_and_run() {
        let tmp = TempDir::new().unwrap();
        let transport = LocalTransport::new(tmp.path());
        let local = tmp.path().join("source.txt");
        std::fs::write(&local, "hello").unwrap();
        transport.copy_file(&local, "/dest/source.txt").unwrap();
        let out = transport.run_command("cat dest/source.txt").unwrap();
        assert_eq!(out.trim(), "hello");
    }
}
