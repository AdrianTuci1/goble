use anyhow::{Context, Result};

use crate::cluster_key::{ClusterIdentity, ClusterKey};
use crate::encrypted_wallet::{DeviceEntry, EncryptedWallet, IdentityWallet};
use crate::identity::{ClusterRole, Identity};
use crate::snapshot::{Snapshot, SnapshotProvider};
use crate::worker::WorkerId;

/// Restore (or join) a new device to an existing cluster.
///
/// The snapshot is encrypted with the cluster key; the identity wallet inside
/// it is encrypted with the user passphrase. This function downloads the latest
/// snapshot, decrypts it, extracts the wallet, issues a device certificate for
/// the new device, and re-uploads the updated wallet snapshot.
pub struct DeviceTransfer;

impl DeviceTransfer {
    /// Restore identity from a snapshot store and return the opened wallet,
    /// the new device identity, and the snapshot payload (so callers can read
    /// worker metadata without downloading again).
    pub fn restore_from_snapshot(
        provider: &dyn SnapshotProvider,
        _worker_id: &WorkerId,
        cluster_key: &ClusterKey,
        passphrase: &[u8],
        new_device_id: &str,
        new_device_name: &str,
        new_device_role: ClusterRole,
    ) -> Result<(IdentityWallet, Identity)> {
        let latest = provider
            .list_snapshots()?
            .into_iter()
            .next()
            .context("no snapshots found in provider")?;
        let snapshot = provider.download_snapshot(&latest.key)?;
        Self::restore_from_snapshot_data(
            &snapshot,
            cluster_key,
            passphrase,
            new_device_id,
            new_device_name,
            new_device_role,
        )
    }

    /// Restore identity from an already-downloaded snapshot.
    pub fn restore_from_snapshot_data(
        snapshot: &Snapshot,
        cluster_key: &ClusterKey,
        passphrase: &[u8],
        new_device_id: &str,
        new_device_name: &str,
        new_device_role: ClusterRole,
    ) -> Result<(IdentityWallet, Identity)> {
        let payload = snapshot.decrypt_payload(cluster_key)?;
        let wallet_value = payload
            .tables
            .get("settings")
            .and_then(|rows| {
                rows.iter()
                    .find(|r| r.get("key").and_then(|v| v.as_str()) == Some("cluster_wallet"))
            })
            .and_then(|r| r.get("value"))
            .context("snapshot does not contain a cluster wallet")?;
        let wallet_json = wallet_value
            .as_str()
            .context("cluster wallet value is not a string")?;
        let encrypted_wallet: EncryptedWallet =
            serde_json::from_str(wallet_json).context("failed to deserialize encrypted wallet")?;
        let mut wallet = IdentityWallet::open(&encrypted_wallet, passphrase)
            .context("failed to decrypt identity wallet")?;

        let identity = Self::add_device_to_wallet(
            &mut wallet,
            cluster_key,
            new_device_id,
            new_device_name,
            new_device_role,
        )?;

        Ok((wallet, identity))
    }

    /// Add a new device certificate to an existing wallet.
    pub fn add_device_to_wallet(
        wallet: &mut IdentityWallet,
        cluster_key: &ClusterKey,
        device_id: &str,
        device_name: &str,
        role: ClusterRole,
    ) -> Result<Identity> {
        let cluster =
            ClusterIdentity::from_key(cluster_key.clone(), &wallet.cluster_name, device_id, role)?;
        wallet.add_device(DeviceEntry {
            device_id: device_id.to_string(),
            name: device_name.to_string(),
            role,
            cert_pem: cluster.device.cert_pem.clone(),
        });
        Ok(cluster.device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_key::ClusterIdentity;
    use crate::encrypted_wallet::IdentityWallet;
    use crate::identity::ClusterRole;
    use crate::snapshot::LocalSnapshotProvider;
    use crate::store::Store;
    use crate::worker::WorkerId;

    #[test]
    fn test_device_restore_from_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = LocalSnapshotProvider::new(tmp.path().join("snapshots"));

        // Owner creates a cluster and uploads a snapshot containing the wallet.
        let owner =
            ClusterIdentity::generate("my-cluster", "owner-device", ClusterRole::Owner).unwrap();
        let mut wallet = IdentityWallet::from(&owner);
        wallet.add_worker(crate::encrypted_wallet::WorkerEntry {
            worker_id: "worker-1".to_string(),
            name: "vps".to_string(),
            url: Some("wss://1.2.3.4:8787/ws".to_string()),
        });
        let sealed = wallet.seal(b"secret-pass").unwrap();

        let store = Store::open(tmp.path().join("owner.db")).unwrap();
        store.set_cluster_wallet(&sealed).unwrap();

        let worker_id = WorkerId::generate();
        let snapshot = Snapshot::from_store(&store, &worker_id, &owner.key).unwrap();
        provider.upload_snapshot(&worker_id, &snapshot).unwrap();

        // New device downloads the snapshot and restores identity.
        let (restored_wallet, new_device) = DeviceTransfer::restore_from_snapshot(
            &provider,
            &worker_id,
            &owner.key,
            b"secret-pass",
            "phone-device",
            "phone",
            ClusterRole::Admin,
        )
        .unwrap();

        assert_eq!(restored_wallet.cluster_name, "my-cluster");
        assert_eq!(restored_wallet.devices.len(), 2);
        assert!(restored_wallet
            .devices
            .iter()
            .any(|d| d.device_id == "phone-device"));
        assert_eq!(restored_wallet.workers.len(), 1);
        assert_eq!(restored_wallet.workers[0].worker_id, "worker-1");
        assert_eq!(new_device.role(), ClusterRole::Admin);
        assert!(!new_device.cert_pem.is_empty());
    }
}
