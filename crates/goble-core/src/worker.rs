use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkerId(pub String);

impl WorkerId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl fmt::Display for WorkerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub id: WorkerId,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub pairing_code: String,
}

impl WorkerConfig {
    pub fn new(
        name: impl Into<String>,
        host: impl Into<String>,
        username: impl Into<String>,
    ) -> Self {
        Self {
            id: WorkerId::generate(),
            name: name.into(),
            host: host.into(),
            port: 7878,
            username: username.into(),
            pairing_code: String::new(),
        }
    }

    pub fn with_pairing_code(mut self, code: impl Into<String>) -> Self {
        self.pairing_code = code.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    Unknown,
    Online,
    Idle,
    Offline,
    Pairing,
    Error(String),
}
