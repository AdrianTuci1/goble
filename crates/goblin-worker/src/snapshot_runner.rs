use std::sync::Arc;
use std::time::Duration;

use goble_core::cluster_key::ClusterKey;
use goble_core::snapshot::{Snapshot, SnapshotProvider};
use goble_core::store::Store;

use crate::state::AppState;

/// Periodically uploads encrypted snapshots and can restore the worker store
/// from the latest snapshot on startup. Snapshots are disaster-recovery only;
/// runtimes are not migrated live between workers.
pub struct SnapshotRunner {
    state: Arc<AppState>,
    provider: Arc<dyn SnapshotProvider>,
    cluster_key: ClusterKey,
    interval: Duration,
    cluster_mode: bool,
}

impl SnapshotRunner {
    pub fn new(
        state: Arc<AppState>,
        provider: Arc<dyn SnapshotProvider>,
        cluster_key: ClusterKey,
        interval: Duration,
        cluster_mode: bool,
    ) -> Self {
        Self {
            state,
            provider,
            cluster_key,
            interval,
            cluster_mode,
        }
    }

    /// If the local store contains no data, download the latest snapshot and
    /// restore it. Returns true if a restore happened. In cluster mode, this
    /// only restores on the very first start (when the PVC database file does not
    /// yet exist). Subsequent restarts keep the local state so that live agents
    /// are not lost.
    pub fn restore_if_empty(&self) -> anyhow::Result<bool> {
        if self.cluster_mode {
            if let Some(path) = self.state.store_path() {
                if path.exists() {
                    return Ok(false);
                }
            }
        }
        let store = self.state.store()?;
        if !is_store_empty(&store) {
            return Ok(false);
        }
        let snapshots = self.provider.list_snapshots()?;
        let Some(latest) = snapshots.into_iter().next() else {
            return Ok(false);
        };
        let snapshot = self.provider.download_snapshot(&latest.key)?;
        snapshot.restore_into_store(&store, &self.cluster_key)?;
        Ok(true)
    }

    /// Spawn a background task that uploads snapshots periodically.
    pub fn start(self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(e) = self.upload_once() {
                    tracing::warn!("snapshot upload failed: {}", e);
                }
            }
        });
    }

    fn upload_once(&self) -> anyhow::Result<()> {
        let store = self.state.store()?;
        let worker_id = self.state.worker_id.clone();
        let snapshot = Snapshot::from_store(&store, &worker_id, &self.cluster_key)?;
        self.provider.upload_snapshot(&worker_id, &snapshot)?;
        Ok(())
    }
}

fn is_store_empty(store: &Store) -> bool {
    match store.export_snapshot_payload() {
        Ok(payload) => payload.tables.values().all(|rows| rows.is_empty()),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::snapshot::LocalSnapshotProvider;
    use goble_core::store::Store;
    use goble_core::worker::WorkerId;

    #[tokio::test]
    async fn test_restore_empty_store() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = Arc::new(LocalSnapshotProvider::new(tmp.path().join("snapshots")));

        let key = ClusterKey::generate();
        let worker_id = WorkerId::generate();

        // Create first worker store and upload a snapshot.
        let store1 = Store::open(tmp.path().join("worker1.db")).unwrap();
        store1.set_setting("hello", "world").unwrap();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();
        provider.upload_snapshot(&worker_id, &snapshot).unwrap();

        // Fresh worker with empty store restores from snapshot.
        let state = AppState::new(worker_id.clone());
        state.set_store_path(tmp.path().join("worker2.db")).unwrap();
        state.set_cluster_key(key.clone());
        state.set_snapshot_provider(provider.clone());

        let runner =
            SnapshotRunner::new(state.clone(), provider, key, Duration::from_secs(60), false);
        let restored = runner.restore_if_empty().unwrap();
        assert!(restored);
        assert_eq!(
            state.store().unwrap().get_setting("hello").unwrap(),
            Some("world".to_string())
        );
    }

    #[tokio::test]
    async fn test_cluster_mode_skips_restore_when_db_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = Arc::new(LocalSnapshotProvider::new(tmp.path().join("snapshots")));

        let key = ClusterKey::generate();
        let worker_id = WorkerId::generate();

        let store1 = Store::open(tmp.path().join("worker1.db")).unwrap();
        store1.set_setting("hello", "world").unwrap();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();
        provider.upload_snapshot(&worker_id, &snapshot).unwrap();

        let state = AppState::new(worker_id.clone());
        state.set_cluster_mode(true);
        // Create an existing DB file on the PVC path.
        let existing_db = tmp.path().join("worker2.db");
        std::fs::write(&existing_db, b"").unwrap();
        state.set_store_path(existing_db).unwrap();
        state.set_cluster_key(key.clone());
        state.set_snapshot_provider(provider.clone());

        let runner =
            SnapshotRunner::new(state.clone(), provider, key, Duration::from_secs(60), true);
        let restored = runner.restore_if_empty().unwrap();
        assert!(!restored);
    }

    #[tokio::test]
    async fn test_cluster_mode_restores_when_db_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let provider = Arc::new(LocalSnapshotProvider::new(tmp.path().join("snapshots")));

        let key = ClusterKey::generate();
        let worker_id = WorkerId::generate();

        let store1 = Store::open(tmp.path().join("worker1.db")).unwrap();
        store1.set_setting("hello", "world").unwrap();
        let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();
        provider.upload_snapshot(&worker_id, &snapshot).unwrap();

        let state = AppState::new(worker_id.clone());
        state.set_cluster_mode(true);
        state.set_store_path(tmp.path().join("worker2.db")).unwrap();
        state.set_cluster_key(key.clone());
        state.set_snapshot_provider(provider.clone());

        let runner =
            SnapshotRunner::new(state.clone(), provider, key, Duration::from_secs(60), true);
        let restored = runner.restore_if_empty().unwrap();
        assert!(restored);
    }
}
