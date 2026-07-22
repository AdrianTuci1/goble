use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::crypto::{decrypt_with_passphrase, encrypt_with_passphrase};

/// Encrypted vault entry stored on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    pub key: String,
    pub encrypted_blob: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

impl VaultEntry {
    pub fn new(key: impl Into<String>, encrypted_blob: Vec<u8>) -> Self {
        Self {
            key: key.into(),
            encrypted_blob,
            metadata: HashMap::new(),
        }
    }
}

/// A credential vault that encrypts each value with a passphrase-derived key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialVault {
    pub entries: HashMap<String, VaultEntry>,
}

impl CredentialVault {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Encrypt and store a value under the given key.
    pub fn set(&mut self, key: impl Into<String>, value: &[u8], passphrase: &[u8]) -> Result<()> {
        let key = key.into();
        let encrypted_blob = encrypt_with_passphrase(value, passphrase)?;
        self.entries
            .insert(key.clone(), VaultEntry::new(key, encrypted_blob));
        Ok(())
    }

    /// Decrypt and return a value by key.
    pub fn get(&self, key: &str, passphrase: &[u8]) -> Result<Option<Vec<u8>>> {
        let entry = match self.entries.get(key) {
            Some(e) => e,
            None => return Ok(None),
        };
        let plaintext = decrypt_with_passphrase(&entry.encrypted_blob, passphrase)
            .with_context(|| format!("failed to decrypt entry '{key}'"))?;
        Ok(Some(plaintext))
    }

    /// Remove a value by key.
    pub fn remove(&mut self, key: &str) -> Result<()> {
        if self.entries.remove(key).is_none() {
            anyhow::bail!("entry not found: {key}");
        }
        Ok(())
    }

    /// List all stored keys.
    pub fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Serialize the vault to bytes (e.g. for disk storage).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("failed to serialize vault")
    }

    /// Deserialize the vault from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("failed to deserialize vault")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let mut vault = CredentialVault::new();
        vault
            .set("openai-api-key", b"sk-123", b"passphrase")
            .unwrap();
        let value = vault.get("openai-api-key", b"passphrase").unwrap();
        assert_eq!(value.unwrap(), b"sk-123");
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let mut vault = CredentialVault::new();
        vault
            .set("openai-api-key", b"sk-123", b"passphrase")
            .unwrap();
        assert!(vault.get("openai-api-key", b"wrong").is_err());
    }

    #[test]
    fn test_missing_key() {
        let vault = CredentialVault::new();
        assert!(vault.get("missing", b"passphrase").unwrap().is_none());
    }

    #[test]
    fn test_remove() {
        let mut vault = CredentialVault::new();
        vault.set("x", b"y", b"p").unwrap();
        assert_eq!(vault.keys(), vec!["x"]);
        vault.remove("x").unwrap();
        assert!(vault.keys().is_empty());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut vault = CredentialVault::new();
        vault.set("x", b"y", b"p").unwrap();
        let bytes = vault.to_bytes().unwrap();
        let loaded = CredentialVault::from_bytes(&bytes).unwrap();
        assert_eq!(loaded.keys(), vec!["x"]);
        assert_eq!(loaded.get("x", b"p").unwrap().unwrap(), b"y");
    }
}
