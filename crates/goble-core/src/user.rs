use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::principal::PrincipalId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: PrincipalId,
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub public_key_pem: Option<String>,
}

impl UserProfile {
    pub fn new(id: PrincipalId, name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            email: email.into(),
            avatar_url: None,
            public_key_pem: None,
        }
    }

    pub fn with_avatar_url(mut self, url: impl Into<String>) -> Self {
        self.avatar_url = Some(url.into());
        self
    }

    pub fn with_public_key(mut self, pem: impl Into<String>) -> Self {
        self.public_key_pem = Some(pem.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedKey {
    pub id: String,
    pub name: String,
    pub public_key_pem: String,
    pub fingerprint: String,
    pub thread_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl AuthorizedKey {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        public_key_pem: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            public_key_pem: public_key_pem.into(),
            fingerprint: fingerprint.into(),
            thread_ids: Vec::new(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserError {
    ProfileNotFound,
    KeyNotFound(String),
    DuplicateKeyFingerprint(String),
}

impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserError::ProfileNotFound => write!(f, "profile not found"),
            UserError::KeyNotFound(id) => write!(f, "authorized key not found: {}", id),
            UserError::DuplicateKeyFingerprint(fp) => {
                write!(f, "duplicate key fingerprint: {}", fp)
            }
        }
    }
}

impl std::error::Error for UserError {}
