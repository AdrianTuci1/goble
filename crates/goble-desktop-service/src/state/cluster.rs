use anyhow::Context;
use base64::Engine;
use goble_core::cluster_key::{ClusterBackup, ClusterIdentity, ClusterKey};
use goble_core::encrypted_wallet::IdentityWallet;
use goble_core::identity::ClusterRole;

use super::{DesktopState, WorkerInvite};

impl DesktopState {
    pub fn export_cluster_key(&self) -> anyhow::Result<String> {
        match self.get_cluster_identity() {
            Some(identity) => Ok(identity.export_key()),
            None => anyhow::bail!("no cluster identity configured"),
        }
    }

    pub fn export_cluster_backup(&self) -> anyhow::Result<ClusterBackup> {
        match self.get_cluster_identity() {
            Some(identity) => identity.export_backup(),
            None => anyhow::bail!("no cluster identity configured"),
        }
    }

    pub fn get_cluster_identity(&self) -> Option<ClusterIdentity> {
        self.cluster_identity.lock().clone()
    }

    pub fn create_cluster(&self, name: &str, passphrase: &str) -> anyhow::Result<ClusterIdentity> {
        let identity = ClusterIdentity::generate(name, &Self::device_id(), ClusterRole::Owner)?;
        self.set_cluster_identity(identity.clone(), passphrase)
    }

    pub fn import_cluster_key(
        &self,
        key_b64: &str,
        name: &str,
        passphrase: &str,
    ) -> anyhow::Result<ClusterIdentity> {
        let key = ClusterKey::from_base64(key_b64)?;
        let identity =
            ClusterIdentity::from_key(key, name, &Self::device_id(), ClusterRole::Admin)?;
        self.set_cluster_identity(identity.clone(), passphrase)
    }

    pub fn export_identity_wallet(&self, passphrase: &str) -> anyhow::Result<String> {
        let identity = self
            .get_cluster_identity()
            .context("no cluster identity unlocked")?;
        let wallet = IdentityWallet::from(&identity);
        let sealed = wallet.seal(passphrase.as_bytes())?;
        Ok(serde_json::to_string(&sealed)?)
    }

    pub fn import_identity_wallet(
        &self,
        wallet_json: &str,
        passphrase: &str,
    ) -> anyhow::Result<ClusterIdentity> {
        let sealed: goble_core::encrypted_wallet::EncryptedWallet =
            serde_json::from_str(wallet_json).context("invalid wallet JSON")?;
        let wallet = IdentityWallet::open(&sealed, passphrase.as_bytes())?;
        let device_id = Self::device_id();
        let identity = wallet.to_cluster_identity(&device_id, ClusterRole::Admin)?;
        *self.cluster_identity.lock() = Some(identity.clone());
        Ok(identity)
    }

    pub fn unlock_cluster_identity(&self, passphrase: &str) -> anyhow::Result<bool> {
        let wallet = self.store.lock().get_cluster_wallet()?;
        match wallet {
            Some(wallet) => {
                let bytes = wallet.open(passphrase.as_bytes())?;
                let identity_wallet: IdentityWallet = serde_json::from_slice(&bytes)?;
                let device_id = Self::device_id();
                let identity =
                    identity_wallet.to_cluster_identity(&device_id, ClusterRole::Admin)?;
                *self.cluster_identity.lock() = Some(identity);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn has_stored_cluster_identity(&self) -> bool {
        self.store
            .lock()
            .get_cluster_wallet()
            .ok()
            .flatten()
            .is_some()
    }

    /// Generate a `helm install` command for a Goblin worker cluster. Requires an
    /// unlocked cluster identity so the worker mTLS bundle and snapshot key can be
    /// embedded in the generated command.
    #[allow(clippy::too_many_arguments)]
    pub fn cluster_helm_install(
        &self,
        name: String,
        namespace: String,
        replicas: u32,
        storage_class: Option<String>,
        persistence_size: String,
        provider: String,
        endpoint: Option<String>,
        bucket: Option<String>,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        region: Option<String>,
        interval_seconds: u64,
        local_chart: Option<String>,
    ) -> anyhow::Result<String> {
        let identity = self
            .get_cluster_identity()
            .context("cluster identity is not unlocked")?;
        let worker_id = format!("{}-0", name);
        let bundle = identity
            .ca
            .sign_worker_bundle(&worker_id, &identity.cluster_name, 365)?;
        let bundle_json = serde_json::to_string(&bundle)?;
        let bundle_b64 = base64::engine::general_purpose::STANDARD.encode(bundle_json);
        let cluster_key_b64 = identity.export_key();

        let chart_ref = local_chart
            .map(|p| format!("{} ", p))
            .unwrap_or_else(|| "goble/goblin-cluster ".to_string());
        let mut parts = vec![
            format!("helm install {} ", name),
            chart_ref,
            format!("--namespace {} --create-namespace ", namespace),
            format!("--set replicas={} ", replicas),
            format!("--set workerBundle={} ", bundle_b64),
            format!("--set clusterKey={} ", cluster_key_b64),
            "--set snapshot.enabled=true ".to_string(),
            format!("--set snapshot.provider={} ", provider),
            format!("--set snapshot.intervalSeconds={} ", interval_seconds),
        ];
        if let Some(region) = region {
            parts.push(format!("--set snapshot.region={} ", region));
        }
        if let Some(endpoint) = endpoint {
            parts.push(format!("--set snapshot.endpoint={} ", endpoint));
        }
        if let Some(bucket) = bucket {
            parts.push(format!("--set snapshot.bucket={} ", bucket));
        }
        if let Some(access_key_id) = access_key_id {
            parts.push(format!("--set snapshot.accessKeyId={} ", access_key_id));
        }
        if let Some(secret_access_key) = secret_access_key {
            parts.push(format!(
                "--set snapshot.secretAccessKey={} ",
                secret_access_key
            ));
        }
        if let Some(storage_class) = storage_class {
            parts.push(format!("--set persistence.storageClass={} ", storage_class));
        }
        parts.push(format!("--set persistence.size={}", persistence_size));
        Ok(parts.join(""))
    }

    /// Install a worker over SSH on a remote Unix host.
    #[cfg(unix)]
    pub fn install_worker_ssh(
        &self,
        creds: crate::ssh_installer::SshCredentials,
        release_tag: &str,
        repo: &str,
        pairing_code: &str,
    ) -> Result<crate::ssh_installer::WorkerInstallResult, crate::ssh_installer::InstallError> {
        let cluster = self
            .get_cluster_identity()
            .ok_or_else(|| crate::ssh_installer::InstallError::Other(
                "no cluster identity configured".to_string(),
            ))?;
        crate::ssh_installer::install_worker(&cluster, &creds, release_tag, repo, pairing_code)
    }

    /// Install a worker over SSH on a remote Unix host.
    #[cfg(not(unix))]
    pub fn install_worker_ssh(
        &self,
        _creds: crate::ssh_installer::SshCredentials,
        _release_tag: &str,
        _repo: &str,
        _pairing_code: &str,
    ) -> Result<crate::ssh_installer::WorkerInstallResult, crate::ssh_installer::InstallError> {
        Err(crate::ssh_installer::InstallError::Other(
            "Remote worker installation requires an SSH client, which is not available on this platform.".to_string(),
        ))
    }

    pub fn generate_worker_invite(&self, worker_id: &str) -> anyhow::Result<WorkerInvite> {
        let identity = self
            .get_cluster_identity()
            .context("no cluster identity unlocked")?;
        let bundle = identity
            .ca
            .sign_worker_bundle(worker_id, &identity.cluster_name, 365)
            .context("failed to sign worker bundle")?;
        let bundle_json = serde_json::to_string(&bundle)?;
        Ok(WorkerInvite {
            worker_id: worker_id.to_string(),
            cluster_key: identity.export_key(),
            bundle: base64::engine::general_purpose::STANDARD.encode(bundle_json),
        })
    }

    fn set_cluster_identity(
        &self,
        identity: ClusterIdentity,
        passphrase: &str,
    ) -> anyhow::Result<ClusterIdentity> {
        let wallet = IdentityWallet::from(&identity);
        let sealed = wallet.seal(passphrase.as_bytes())?;
        self.store.lock().set_cluster_wallet(&sealed)?;
        *self.cluster_identity.lock() = Some(identity.clone());
        self.emit("cluster:updated", ());
        Ok(identity)
    }
}
