use std::sync::Arc;

use goble_desktop_service::{ClusterIdentityInfo, DesktopState};

pub fn get_cluster_identity(state: &Arc<DesktopState>) -> Option<ClusterIdentityInfo> {
    state.get_cluster_identity().map(|i| ClusterIdentityInfo {
        cluster_name: i.cluster_name,
        ca_cert_pem: i.ca.identity.cert_pem,
        device_serial: i.device.serial().to_string(),
    })
}

pub struct CreateClusterRequest {
    pub name: String,
    pub passphrase: String,
}

pub fn create_cluster(
    state: &Arc<DesktopState>,
    req: CreateClusterRequest,
) -> anyhow::Result<ClusterIdentityInfo> {
    let identity = state.create_cluster(&req.name, &req.passphrase)?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

pub struct ImportClusterKeyRequest {
    pub key: String,
    pub name: String,
    pub passphrase: String,
}

pub fn import_cluster_key(
    state: &Arc<DesktopState>,
    req: ImportClusterKeyRequest,
) -> anyhow::Result<ClusterIdentityInfo> {
    let identity = state.import_cluster_key(&req.key, &req.name, &req.passphrase)?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

pub fn export_cluster_key(state: &Arc<DesktopState>) -> anyhow::Result<String> {
    state.export_cluster_key()
}

pub fn export_cluster_backup(state: &Arc<DesktopState>) -> anyhow::Result<serde_json::Value> {
    let backup = state.export_cluster_backup()?;
    serde_json::to_value(&backup).map_err(|e| anyhow::anyhow!("{e}"))
}

pub struct ExportIdentityRequest {
    pub passphrase: String,
}

pub fn export_identity_wallet(
    state: &Arc<DesktopState>,
    req: ExportIdentityRequest,
) -> anyhow::Result<String> {
    state.export_identity_wallet(&req.passphrase)
}

pub struct ImportIdentityRequest {
    pub wallet: String,
    pub passphrase: String,
}

pub fn import_identity_wallet(
    state: &Arc<DesktopState>,
    req: ImportIdentityRequest,
) -> anyhow::Result<ClusterIdentityInfo> {
    let identity = state.import_identity_wallet(&req.wallet, &req.passphrase)?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

pub fn unlock_cluster_identity(state: &Arc<DesktopState>, passphrase: &str) -> anyhow::Result<bool> {
    state.unlock_cluster_identity(passphrase)
}

pub fn has_cluster_identity(state: &Arc<DesktopState>) -> bool {
    state.has_stored_cluster_identity()
}

pub struct ClusterHelmInstallRequest {
    pub name: String,
    pub namespace: String,
    pub replicas: u32,
    pub storage_class: Option<String>,
    pub persistence_size: String,
    pub provider: String,
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub region: Option<String>,
    pub interval_seconds: u64,
    pub local_chart: Option<String>,
}

pub fn cluster_helm_install(
    state: &Arc<DesktopState>,
    req: ClusterHelmInstallRequest,
) -> anyhow::Result<String> {
    state.cluster_helm_install(
        req.name,
        req.namespace,
        req.replicas,
        req.storage_class,
        req.persistence_size,
        req.provider,
        req.endpoint,
        req.bucket,
        req.access_key_id,
        req.secret_access_key,
        req.region,
        req.interval_seconds,
        req.local_chart,
    )
}
