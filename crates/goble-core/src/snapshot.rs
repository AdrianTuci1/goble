use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cluster_key::ClusterKey;
use crate::crypto;
use crate::store::Store;
use crate::worker::WorkerId;

/// Current snapshot format version. Bumped only when the plaintext schema
/// changes in a backward-incompatible way.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Encrypted disaster-recovery snapshot produced by a worker. The header is
/// plaintext so a restore operation can identify the snapshot; the payload is
/// encrypted with the backup key derived from the cluster key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub worker_id: String,
    pub key_fingerprint: String,
    pub ciphertext: Vec<u8>,
}

/// Plaintext payload inside a snapshot. Kept separate from the encrypted
/// container so serialization format can be changed independently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotPayload {
    pub version: u32,
    pub tables: HashMap<String, Vec<serde_json::Map<String, serde_json::Value>>>,
}

impl SnapshotPayload {
    pub fn new() -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            tables: HashMap::new(),
        }
    }
}

impl Snapshot {
    /// Build a snapshot from a `Store` and encrypt it with the backup key
    /// derived from the supplied cluster key.
    pub fn from_store(
        store: &Store,
        worker_id: &WorkerId,
        cluster_key: &ClusterKey,
    ) -> Result<Self> {
        let payload = store.export_snapshot_payload()?;
        let plaintext = serde_json::to_vec(&payload).context("failed to serialize snapshot")?;
        let key = cluster_key.derive_backup_key();
        let ciphertext = crypto::encrypt(&plaintext, &key)?;
        let fingerprint = compute_key_fingerprint(&key);

        Ok(Self {
            version: SNAPSHOT_VERSION,
            created_at: Utc::now(),
            worker_id: worker_id.to_string(),
            key_fingerprint: fingerprint,
            ciphertext,
        })
    }

    /// Decrypt the snapshot with the backup key derived from the cluster key and
    /// return the plaintext payload. The fingerprint is verified first to
    /// produce a clear error when the wrong key is used.
    pub fn decrypt_payload(&self, cluster_key: &ClusterKey) -> Result<SnapshotPayload> {
        let key = cluster_key.derive_backup_key();
        let fingerprint = compute_key_fingerprint(&key);
        if fingerprint != self.key_fingerprint {
            anyhow::bail!("snapshot fingerprint mismatch: wrong cluster key");
        }
        let plaintext = crypto::decrypt(&self.ciphertext, &key)?;
        let payload: SnapshotPayload =
            serde_json::from_slice(&plaintext).context("failed to deserialize snapshot")?;
        if payload.version != SNAPSHOT_VERSION {
            anyhow::bail!("unsupported snapshot payload version {}", payload.version);
        }
        Ok(payload)
    }

    /// Restore the snapshot into a `Store`.
    pub fn restore_into_store(&self, store: &Store, cluster_key: &ClusterKey) -> Result<()> {
        let payload = self.decrypt_payload(cluster_key)?;
        store.import_snapshot_payload(payload)?;
        Ok(())
    }
}

fn compute_key_fingerprint(key: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hex::encode(&hasher.finalize()[..8])
}

/// External storage for encrypted snapshots. Providers are currently synchronous;
/// async I/O can be wrapped in `tokio::task::spawn_blocking` by callers.
pub trait SnapshotProvider: Send + Sync {
    fn list_snapshots(&self) -> Result<Vec<SnapshotEntry>>;
    fn upload_snapshot(&self, worker_id: &WorkerId, snapshot: &Snapshot) -> Result<SnapshotEntry>;
    fn download_snapshot(&self, key: &str) -> Result<Snapshot>;
}

/// Metadata for a stored snapshot object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub key: String,
    pub worker_id: String,
    pub created_at: DateTime<Utc>,
    pub size: usize,
}

/// Local filesystem provider. Useful for tests and for storing snapshots on a
/// mounted NAS or host path.
#[derive(Debug, Clone)]
pub struct LocalSnapshotProvider {
    pub root: std::path::PathBuf,
}

impl LocalSnapshotProvider {
    pub fn new<P: Into<std::path::PathBuf>>(root: P) -> Self {
        Self { root: root.into() }
    }

    fn entry_for(&self, path: &std::path::Path) -> Result<SnapshotEntry> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let parts: Vec<&str> = name.splitn(3, '_').collect();
        let worker_id = parts.get(1).unwrap_or(&"").to_string();
        let meta = std::fs::metadata(path).context("failed to read snapshot metadata")?;
        let created_at = DateTime::from_timestamp(meta.modified()?.elapsed()?.as_secs() as i64, 0)
            .unwrap_or_else(Utc::now);
        Ok(SnapshotEntry {
            key: path.to_string_lossy().to_string(),
            worker_id,
            created_at,
            size: meta.len() as usize,
        })
    }
}

impl SnapshotProvider for LocalSnapshotProvider {
    fn list_snapshots(&self) -> Result<Vec<SnapshotEntry>> {
        let mut entries = Vec::new();
        if !self.root.exists() {
            return Ok(entries);
        }
        for entry in std::fs::read_dir(&self.root).context("failed to read snapshot directory")? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("snapshot") {
                entries.push(self.entry_for(&path)?);
            }
        }
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(entries)
    }

    fn upload_snapshot(&self, worker_id: &WorkerId, snapshot: &Snapshot) -> Result<SnapshotEntry> {
        std::fs::create_dir_all(&self.root).context("failed to create snapshot directory")?;
        let key = format!(
            "{}/goble_{}_{}.snapshot",
            self.root.display(),
            worker_id,
            snapshot.created_at.timestamp()
        );
        let bytes =
            serde_json::to_vec(snapshot).context("failed to serialize snapshot for upload")?;
        std::fs::write(&key, &bytes).context("failed to write snapshot")?;
        Ok(SnapshotEntry {
            key,
            worker_id: worker_id.to_string(),
            created_at: snapshot.created_at,
            size: bytes.len(),
        })
    }

    fn download_snapshot(&self, key: &str) -> Result<Snapshot> {
        let bytes = std::fs::read(key).context("failed to read snapshot")?;
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).context("failed to deserialize snapshot")?;
        Ok(snapshot)
    }
}

/// S3-compatible provider (placeholder). Will be wired to S3/R2/MinIO in a
/// follow-up that adds AWS Signature V4 signing without pulling in the AWS SDK.
#[derive(Debug, Clone)]
pub struct S3SnapshotProvider;

impl S3SnapshotProvider {
    pub fn new(_bucket: &str, _prefix: &str) -> Self {
        Self
    }
}

impl SnapshotProvider for S3SnapshotProvider {
    fn list_snapshots(&self) -> Result<Vec<SnapshotEntry>> {
        anyhow::bail!("S3 snapshot provider is not implemented yet")
    }

    fn upload_snapshot(
        &self,
        _worker_id: &WorkerId,
        _snapshot: &Snapshot,
    ) -> Result<SnapshotEntry> {
        anyhow::bail!("S3 snapshot provider is not implemented yet")
    }

    fn download_snapshot(&self, _key: &str) -> Result<Snapshot> {
        anyhow::bail!("S3 snapshot provider is not implemented yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn test_snapshot_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store1 = Store::open(tmp.path().join("store1.sqlite")).unwrap();
        store1.set_setting("hello", "world").unwrap();

        let worker_id = WorkerId::generate();
        let key = ClusterKey::generate();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();

        let store2 = Store::open(tmp.path().join("store2.sqlite")).unwrap();
        snapshot.restore_into_store(&store2, &key).unwrap();

        assert_eq!(
            store2.get_setting("hello").unwrap(),
            Some("world".to_string())
        );
    }

    #[test]
    fn test_wrong_key_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let store1 = Store::open(tmp.path().join("store1.sqlite")).unwrap();
        store1.set_setting("hello", "world").unwrap();

        let worker_id = WorkerId::generate();
        let key = ClusterKey::generate();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();

        let wrong_key = ClusterKey::generate();
        assert!(snapshot.restore_into_store(&store1, &wrong_key).is_err());
    }

    #[test]
    fn test_local_provider_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = LocalSnapshotProvider::new(tmp.path().join("snapshots"));
        let store = Store::open_in_memory().unwrap();
        store.set_setting("hello", "world").unwrap();

        let worker_id = WorkerId::generate();
        let key = ClusterKey::generate();
        let snapshot = Snapshot::from_store(&store, &worker_id, &key).unwrap();
        let entry = provider.upload_snapshot(&worker_id, &snapshot).unwrap();

        let listed = provider.list_snapshots().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, entry.key);

        let downloaded = provider.download_snapshot(&entry.key).unwrap();
        let store2 = Store::open_in_memory().unwrap();
        downloaded.restore_into_store(&store2, &key).unwrap();
        assert_eq!(
            store2.get_setting("hello").unwrap(),
            Some("world".to_string())
        );
    }
}
