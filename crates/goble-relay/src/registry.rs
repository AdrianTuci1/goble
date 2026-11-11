//! Durable relay registry: devices, workers and revocations.
//!
//! The registry outlives a relay process. A restart keeps the devices and
//! workers it knows, so a client does not have to pair again and a worker does
//! not have to re-register just because the relay was redeployed.
//!
//! The relay stores public material only: names, ids, certificate serials,
//! roles, tags and revocation reasons. Private keys stay in device keystores
//! and worker bundles.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use goble_core::identity::ClusterRole;
use rusqlite::{params, Connection, OptionalExtension, Row};

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("registry database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("the registry lock is poisoned")]
    Lock,
    #[error("unknown device {0}")]
    UnknownDevice(String),
    #[error("unknown worker {0}")]
    UnknownWorker(String),
    #[error("the certificate serial does not belong to {0}")]
    SerialMismatch(String),
    #[error("certificate serial {0} is revoked")]
    Revoked(String),
    #[error("a record already uses certificate serial {0}")]
    SerialConflict(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device_id: String,
    pub serial: String,
    pub role: ClusterRole,
    pub name: String,
    /// URL-safe base64 Ed25519 public key, when the device has one.
    pub public_key: String,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRecord {
    pub worker_id: String,
    pub serial: String,
    pub name: String,
    pub address: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub revoked: bool,
}

/// A SQLite-backed registry. One connection behind a mutex is enough for one
/// relay node; a cluster replaces the storage layer with Postgres and keeps the
/// same record types.
pub struct Registry {
    connection: Mutex<Connection>,
}

impl Registry {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, RegistryError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, RegistryError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS relay_devices (
                device_id  TEXT PRIMARY KEY,
                serial     TEXT NOT NULL UNIQUE,
                role       TEXT NOT NULL,
                name       TEXT NOT NULL,
                public_key TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                last_seen  INTEGER,
                revoked    INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS relay_workers (
                worker_id  TEXT PRIMARY KEY,
                serial     TEXT NOT NULL UNIQUE,
                name       TEXT NOT NULL,
                address    TEXT NOT NULL,
                tags       TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_seen  INTEGER,
                revoked    INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS relay_revocations (
                serial     TEXT PRIMARY KEY,
                reason     TEXT NOT NULL,
                revoked_at INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, RegistryError> {
        self.connection.lock().map_err(|_| RegistryError::Lock)
    }

    /// Insert or update a device. The creation time is kept on update; a
    /// revoked serial is not cleared by re-registering the device.
    pub fn register_device(&self, device: &DeviceRecord) -> Result<(), RegistryError> {
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                INSERT INTO relay_devices
                    (device_id, serial, role, name, public_key, created_at, last_seen, revoked)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(device_id) DO UPDATE SET
                    serial = excluded.serial,
                    role = excluded.role,
                    name = excluded.name,
                    public_key = excluded.public_key,
                    last_seen = excluded.last_seen,
                    revoked = excluded.revoked
                "#,
                params![
                    device.device_id,
                    device.serial,
                    device.role.to_string(),
                    device.name,
                    device.public_key,
                    device.created_at,
                    device.last_seen,
                    device.revoked as i64,
                ],
            )
            .map_err(|error| map_serial_conflict(error, &device.serial))?;
        Ok(())
    }

    pub fn device(&self, device_id: &str) -> Result<Option<DeviceRecord>, RegistryError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT device_id, serial, role, name, public_key, created_at, last_seen, revoked
                 FROM relay_devices WHERE device_id = ?1",
                [device_id],
                device_from_row,
            )
            .optional()
            .map_err(RegistryError::from)
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceRecord>, RegistryError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT device_id, serial, role, name, public_key, created_at, last_seen, revoked
             FROM relay_devices ORDER BY name, device_id",
        )?;
        let rows = statement.query_map([], device_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Authenticate a device connection: the id and serial must name the same
    /// record and neither the record nor the serial may be revoked. A
    /// successful check refreshes `last_seen`.
    pub fn authorize_device(
        &self,
        device_id: &str,
        serial: &str,
        now: i64,
    ) -> Result<DeviceRecord, RegistryError> {
        let mut device = self
            .device(device_id)?
            .ok_or_else(|| RegistryError::UnknownDevice(device_id.to_string()))?;
        if device.serial != serial {
            return Err(RegistryError::SerialMismatch(device_id.to_string()));
        }
        if device.revoked || self.is_revoked(serial)? {
            return Err(RegistryError::Revoked(serial.to_string()));
        }
        device.last_seen = Some(now);
        let connection = self.connection()?;
        connection.execute(
            "UPDATE relay_devices SET last_seen = ?1 WHERE device_id = ?2",
            params![now, device_id],
        )?;
        Ok(device)
    }

    /// Insert or update a worker. The creation time is kept on update.
    pub fn register_worker(&self, worker: &WorkerRecord) -> Result<(), RegistryError> {
        let tags = serde_json::to_string(&worker.tags)
            .map_err(|_| RegistryError::Database(rusqlite::Error::InvalidQuery))?;
        let connection = self.connection()?;
        connection
            .execute(
                r#"
                INSERT INTO relay_workers
                    (worker_id, serial, name, address, tags, created_at, last_seen, revoked)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(worker_id) DO UPDATE SET
                    serial = excluded.serial,
                    name = excluded.name,
                    address = excluded.address,
                    tags = excluded.tags,
                    last_seen = excluded.last_seen,
                    revoked = excluded.revoked
                "#,
                params![
                    worker.worker_id,
                    worker.serial,
                    worker.name,
                    worker.address,
                    tags,
                    worker.created_at,
                    worker.last_seen,
                    worker.revoked as i64,
                ],
            )
            .map_err(|error| map_serial_conflict(error, &worker.serial))?;
        Ok(())
    }

    pub fn worker(&self, worker_id: &str) -> Result<Option<WorkerRecord>, RegistryError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT worker_id, serial, name, address, tags, created_at, last_seen, revoked
                 FROM relay_workers WHERE worker_id = ?1",
                [worker_id],
                worker_from_row,
            )
            .optional()
            .map_err(RegistryError::from)
    }

    pub fn list_workers(&self) -> Result<Vec<WorkerRecord>, RegistryError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT worker_id, serial, name, address, tags, created_at, last_seen, revoked
             FROM relay_workers ORDER BY name, worker_id",
        )?;
        let rows = statement.query_map([], worker_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Authenticate a worker connection and refresh its `last_seen`.
    pub fn authorize_worker(
        &self,
        worker_id: &str,
        serial: &str,
        now: i64,
    ) -> Result<WorkerRecord, RegistryError> {
        let mut worker = self
            .worker(worker_id)?
            .ok_or_else(|| RegistryError::UnknownWorker(worker_id.to_string()))?;
        if worker.serial != serial {
            return Err(RegistryError::SerialMismatch(worker_id.to_string()));
        }
        if worker.revoked || self.is_revoked(serial)? {
            return Err(RegistryError::Revoked(serial.to_string()));
        }
        worker.last_seen = Some(now);
        let connection = self.connection()?;
        connection.execute(
            "UPDATE relay_workers SET last_seen = ?1 WHERE worker_id = ?2",
            params![now, worker_id],
        )?;
        Ok(worker)
    }

    /// Revoke a certificate serial. Any device or worker holding it is marked
    /// revoked and every later authorization fails.
    pub fn revoke(&self, serial: &str, reason: &str, now: i64) -> Result<bool, RegistryError> {
        let connection = self.connection()?;
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO relay_revocations (serial, reason, revoked_at)
             VALUES (?1, ?2, ?3)",
            params![serial, reason, now],
        )?;
        connection.execute(
            "UPDATE relay_devices SET revoked = 1 WHERE serial = ?1",
            [serial],
        )?;
        connection.execute(
            "UPDATE relay_workers SET revoked = 1 WHERE serial = ?1",
            [serial],
        )?;
        Ok(inserted > 0)
    }

    pub fn is_revoked(&self, serial: &str) -> Result<bool, RegistryError> {
        let connection = self.connection()?;
        let found: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM relay_revocations WHERE serial = ?1",
                [serial],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    pub fn revoked_serials(&self) -> Result<Vec<String>, RegistryError> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT serial FROM relay_revocations ORDER BY revoked_at")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The row count is useful to a relay's health endpoint and to tests that
    /// assert a restart did not lose state.
    pub fn counts(&self) -> Result<(usize, usize, usize), RegistryError> {
        let connection = self.connection()?;
        let devices: i64 =
            connection.query_row("SELECT COUNT(*) FROM relay_devices", [], |row| row.get(0))?;
        let workers: i64 =
            connection.query_row("SELECT COUNT(*) FROM relay_workers", [], |row| row.get(0))?;
        let revoked: i64 =
            connection.query_row("SELECT COUNT(*) FROM relay_revocations", [], |row| {
                row.get(0)
            })?;
        Ok((devices as usize, workers as usize, revoked as usize))
    }
}

fn device_from_row(row: &Row<'_>) -> rusqlite::Result<DeviceRecord> {
    Ok(DeviceRecord {
        device_id: row.get(0)?,
        serial: row.get(1)?,
        role: role_from_row(row, 2)?,
        name: row.get(3)?,
        public_key: row.get(4)?,
        created_at: row.get(5)?,
        last_seen: row.get(6)?,
        revoked: row.get::<_, i64>(7)? != 0,
    })
}

fn worker_from_row(row: &Row<'_>) -> rusqlite::Result<WorkerRecord> {
    let tags: String = row.get(4)?;
    Ok(WorkerRecord {
        worker_id: row.get(0)?,
        serial: row.get(1)?,
        name: row.get(2)?,
        address: row.get(3)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        created_at: row.get(5)?,
        last_seen: row.get(6)?,
        revoked: row.get::<_, i64>(7)? != 0,
    })
}

fn role_from_row(row: &Row<'_>, index: usize) -> rusqlite::Result<ClusterRole> {
    let value: String = row.get(index)?;
    value
        .parse()
        .map_err(|_| rusqlite::Error::InvalidColumnType(index, value, rusqlite::types::Type::Text))
}

fn map_serial_conflict(error: rusqlite::Error, serial: &str) -> RegistryError {
    match &error {
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            RegistryError::SerialConflict(serial.to_string())
        }
        _ => RegistryError::Database(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn device(id: &str, serial: &str) -> DeviceRecord {
        DeviceRecord {
            device_id: id.into(),
            serial: serial.into(),
            role: ClusterRole::Operator,
            name: format!("{id} name"),
            public_key: "cHVibGlj".into(),
            created_at: 100,
            last_seen: None,
            revoked: false,
        }
    }

    fn worker(id: &str, serial: &str) -> WorkerRecord {
        WorkerRecord {
            worker_id: id.into(),
            serial: serial.into(),
            name: format!("{id} name"),
            address: "wss://worker.example/v1".into(),
            tags: vec!["builder".into(), "linux".into()],
            created_at: 200,
            last_seen: None,
            revoked: false,
        }
    }

    #[test]
    fn a_device_round_trips_through_the_registry() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_device(&device("phone", "serial-1"))
            .unwrap();
        let stored = registry.device("phone").unwrap().unwrap();
        assert_eq!(stored.device_id, "phone");
        assert_eq!(stored.role, ClusterRole::Operator);
        assert_eq!(stored.serial, "serial-1");
        assert_eq!(stored.public_key, "cHVibGlj");
        assert!(!stored.revoked);
    }

    #[test]
    fn a_worker_keeps_its_tags_and_address() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_worker(&worker("builder-1", "ws-1"))
            .unwrap();
        let stored = registry.worker("builder-1").unwrap().unwrap();
        assert_eq!(stored.tags, vec!["builder", "linux"]);
        assert_eq!(stored.address, "wss://worker.example/v1");
    }

    #[test]
    fn a_restart_keeps_devices_workers_and_revocations() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("relay.db");
        {
            let registry = Registry::open(&path).unwrap();
            registry
                .register_device(&device("phone", "serial-1"))
                .unwrap();
            registry
                .register_worker(&worker("builder", "ws-1"))
                .unwrap();
            registry.revoke("serial-old", "lost", 300).unwrap();
            assert_eq!(registry.counts().unwrap(), (1, 1, 1));
        }
        let registry = Registry::open(&path).unwrap();
        assert_eq!(registry.counts().unwrap(), (1, 1, 1));
        assert!(registry.is_revoked("serial-old").unwrap());
        assert!(registry.device("phone").unwrap().is_some());
        assert!(registry.worker("builder").unwrap().is_some());
    }

    #[test]
    fn registering_a_device_updates_it_without_losing_created_at() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_device(&device("phone", "serial-1"))
            .unwrap();
        let mut changed = device("phone", "serial-1");
        changed.name = "new name".into();
        changed.created_at = 999;
        registry.register_device(&changed).unwrap();
        let stored = registry.device("phone").unwrap().unwrap();
        assert_eq!(stored.name, "new name");
        assert_eq!(stored.created_at, 100);
    }

    #[test]
    fn a_serial_cannot_belong_to_two_devices() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_device(&device("phone", "serial-1"))
            .unwrap();
        let other = device("tablet", "serial-1");
        assert!(matches!(
            registry.register_device(&other),
            Err(RegistryError::SerialConflict(serial)) if serial == "serial-1"
        ));
    }

    #[test]
    fn authorization_checks_the_serial_and_refreshes_last_seen() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_device(&device("phone", "serial-1"))
            .unwrap();
        let stored = registry.authorize_device("phone", "serial-1", 500).unwrap();
        assert_eq!(stored.last_seen, Some(500));
        assert_eq!(
            registry.device("phone").unwrap().unwrap().last_seen,
            Some(500)
        );
        assert!(matches!(
            registry.authorize_device("phone", "serial-2", 501),
            Err(RegistryError::SerialMismatch(_))
        ));
    }

    #[test]
    fn an_unknown_device_is_refused() {
        let registry = Registry::in_memory().unwrap();
        assert!(matches!(
            registry.authorize_device("ghost", "serial", 1),
            Err(RegistryError::UnknownDevice(_))
        ));
        assert!(registry.device("ghost").unwrap().is_none());
    }

    #[test]
    fn a_revoked_device_is_refused_and_stays_refused() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_device(&device("phone", "serial-1"))
            .unwrap();
        assert!(registry.revoke("serial-1", "lost phone", 600).unwrap());
        assert!(matches!(
            registry.authorize_device("phone", "serial-1", 601),
            Err(RegistryError::Revoked(_))
        ));
        assert_eq!(registry.revoked_serials().unwrap(), vec!["serial-1"]);
        // Re-registering must not clear the revocation.
        registry
            .register_device(&device("phone", "serial-1"))
            .unwrap();
        assert!(matches!(
            registry.authorize_device("phone", "serial-1", 602),
            Err(RegistryError::Revoked(_))
        ));
    }

    #[test]
    fn a_revoked_worker_is_refused() {
        let registry = Registry::in_memory().unwrap();
        registry
            .register_worker(&worker("builder", "ws-1"))
            .unwrap();
        registry.revoke("ws-1", "decommissioned", 700).unwrap();
        assert!(matches!(
            registry.authorize_worker("builder", "ws-1", 701),
            Err(RegistryError::Revoked(_))
        ));
    }

    #[test]
    fn revoking_twice_reports_only_the_first_insert() {
        let registry = Registry::in_memory().unwrap();
        assert!(registry.revoke("serial-1", "lost", 1).unwrap());
        assert!(!registry.revoke("serial-1", "lost again", 2).unwrap());
        assert_eq!(registry.revoked_serials().unwrap(), vec!["serial-1"]);
    }

    #[test]
    fn a_cluster_certificate_serial_authorizes_and_can_be_revoked() {
        use goble_core::cluster_key::ClusterKey;
        use goble_core::identity::ClusterCa;

        let registry = Registry::in_memory().unwrap();
        let ca = ClusterCa::from_key(&ClusterKey::generate(), "test-cluster").unwrap();
        let identity = ca.sign_device("phone", ClusterRole::Operator, 365).unwrap();
        let record = DeviceRecord {
            device_id: "phone".into(),
            serial: identity.serial().to_string(),
            role: identity.role(),
            name: "test phone".into(),
            public_key: "cHVibGlj".into(),
            created_at: 1,
            last_seen: None,
            revoked: false,
        };
        registry.register_device(&record).unwrap();
        let authorized = registry
            .authorize_device("phone", identity.serial(), 2)
            .unwrap();
        assert_eq!(authorized.role, ClusterRole::Operator);
        assert_eq!(authorized.serial, identity.serial());

        registry.revoke(identity.serial(), "lost", 3).unwrap();
        assert!(matches!(
            registry.authorize_device("phone", identity.serial(), 4),
            Err(RegistryError::Revoked(_))
        ));
    }

    #[test]
    fn workers_and_devices_are_listed_by_name() {
        let registry = Registry::in_memory().unwrap();
        registry.register_device(&device("zeta", "s-z")).unwrap();
        registry.register_device(&device("alpha", "s-a")).unwrap();
        registry.register_worker(&worker("zeta", "w-z")).unwrap();
        registry.register_worker(&worker("alpha", "w-a")).unwrap();
        let devices: Vec<_> = registry
            .list_devices()
            .unwrap()
            .into_iter()
            .map(|d| d.device_id)
            .collect();
        let workers: Vec<_> = registry
            .list_workers()
            .unwrap()
            .into_iter()
            .map(|w| w.worker_id)
            .collect();
        assert_eq!(devices, vec!["alpha", "zeta"]);
        assert_eq!(workers, vec!["alpha", "zeta"]);
    }
}
