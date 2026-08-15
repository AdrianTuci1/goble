use std::io::Write;
use std::process::{Command, Stdio};

use goble_core::cluster_key::ClusterIdentity;
use goble_core::provision::{provision_worker, ProvisionConfig, SshTransport};

#[derive(Debug, Clone)]
pub struct SshCredentials {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub private_key: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkerInstallResult {
    pub platform: PlatformInfo,
    pub asset_url: String,
    pub install_log: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub family: String,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("ssh command failed: {0}")]
    Ssh(String),
    #[error("platform detection failed: {0}")]
    PlatformDetection(String),
    #[error("asset selection failed: {0}")]
    AssetSelection(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("other: {0}")]
    Other(String),
}

pub fn detect_platform(creds: &SshCredentials) -> Result<PlatformInfo, InstallError> {
    let output = run_ssh(
        creds,
        &["curl", "-fsS", "http://localhost:8787/platform"],
        None,
    )?;
    if output.status.success() {
        let info: PlatformInfo = serde_json::from_slice(&output.stdout)
            .map_err(|e| InstallError::PlatformDetection(e.to_string()))?;
        Ok(info)
    } else {
        Err(InstallError::PlatformDetection(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

pub fn resolve_worker_asset(
    platform: &PlatformInfo,
    release_tag: &str,
    repo: &str,
) -> Result<String, InstallError> {
    let arch = match platform.arch.as_str() {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => {
            return Err(InstallError::AssetSelection(format!(
                "unsupported arch: {}",
                other
            )))
        }
    };
    let asset_name = format!("goblin-worker-{}_unknown_linux_gnu.tar.gz", arch);
    Ok(format!(
        "https://github.com/{}/releases/download/{}/{}",
        repo, release_tag, asset_name
    ))
}

pub fn install_worker(
    cluster: &ClusterIdentity,
    creds: &SshCredentials,
    release_tag: &str,
    repo: &str,
    pairing_code: &str,
) -> Result<WorkerInstallResult, InstallError> {
    let platform = detect_platform(creds)?;
    let asset_url = resolve_worker_asset(&platform, release_tag, repo)?;

    let local_binary =
        std::env::temp_dir().join(format!("goblin-{}-{}", release_tag, platform.arch));
    // Download the worker binary locally so we can copy it via the provision transport.
    let download_output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            &asset_url,
            "-o",
            &local_binary.display().to_string(),
        ])
        .output()
        .map_err(InstallError::Io)?;
    if !download_output.status.success() {
        return Err(InstallError::Ssh(format!(
            "failed to download asset: {}",
            String::from_utf8_lossy(&download_output.stderr)
        )));
    }

    let mut cmd = std::process::Command::new("tar");
    cmd.args([
        "-xzf",
        &local_binary.display().to_string(),
        "-C",
        &std::env::temp_dir().display().to_string(),
    ]);
    let tar_output = cmd.output().map_err(InstallError::Io)?;
    if !tar_output.status.success() {
        return Err(InstallError::Ssh(format!(
            "failed to extract asset: {}",
            String::from_utf8_lossy(&tar_output.stderr)
        )));
    }

    let extracted_binary = std::env::temp_dir().join("goblin");

    let worker_id = uuid::Uuid::new_v4().to_string();
    let pairing_hash = goble_core::crypto::hash_pairing_code(pairing_code, &[0u8; 16])
        .map_err(|e| InstallError::Other(format!("failed to hash pairing code: {e}")))?;
    let config = ProvisionConfig::from_cluster_identity(
        cluster,
        &worker_id,
        &worker_id,
        &creds.host,
        "/opt/goblin",
        pairing_hash,
        extracted_binary,
    )
    .map_err(|e| InstallError::Other(format!("failed to build provision config: {e}")))?;

    let transport = SshTransport::new(&creds.host, &creds.user, None);
    provision_worker(&transport, &config).map_err(|e| InstallError::Ssh(e.to_string()))?;

    Ok(WorkerInstallResult {
        platform,
        asset_url,
        install_log: format!("provisioned worker {worker_id} at /opt/goblin"),
    })
}

fn run_ssh(
    creds: &SshCredentials,
    remote_cmd: &[&str],
    stdin: Option<&[u8]>,
) -> Result<std::process::Output, InstallError> {
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
        .arg("-")
        .arg(format!("{}@{}", creds.user, creds.host))
        .args(remote_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    if let Some(data) = stdin {
        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin.write_all(data)?;
        }
    }
    let output = child.wait_with_output()?;
    Ok(output)
}
