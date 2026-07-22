use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;

use crate::crypto::{decrypt_with_passphrase, encrypt_with_passphrase};
use crate::secret::{Secret, SecretStore};

/// High-level secrets manager: stores encrypted values and decrypts on demand.
pub struct SecretManager<S: SecretStore> {
    store: Arc<Mutex<S>>,
    master_key: Arc<Mutex<Vec<u8>>>,
}

impl<S: SecretStore> SecretManager<S> {
    pub fn new(store: S, master_key: Vec<u8>) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            master_key: Arc::new(Mutex::new(master_key)),
        }
    }

    fn master_key(&self) -> Vec<u8> {
        self.master_key.lock().clone()
    }

    pub fn set(&self, name: &str, provider: &str, plaintext: &str) -> Result<String> {
        let encrypted = encrypt_with_passphrase(plaintext.as_bytes(), &self.master_key())
            .context("failed to encrypt secret")?;
        let secret = Secret::new(name, provider, encrypted);
        let id = secret.id.clone();
        self.store.lock().insert(secret)?;
        Ok(id)
    }

    pub fn get_decrypted(&self, id: &str) -> Result<Option<(Secret, String)>> {
        let store = self.store.lock();
        let secret = store.get(id);
        drop(store);
        match secret {
            Some(s) => {
                let plaintext = decrypt_with_passphrase(&s.encrypted_value, &self.master_key())
                    .context("failed to decrypt secret")?;
                let value = String::from_utf8(plaintext).context("secret is not valid utf-8")?;
                Ok(Some((s, value)))
            }
            None => Ok(None),
        }
    }

    pub fn list(&self) -> Vec<Secret> {
        self.store.lock().list()
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        self.store.lock().remove(id)?;
        Ok(())
    }

    pub fn rotate_key(&self, new_master_key: Vec<u8>) -> Result<()> {
        let mut store = self.store.lock();
        let old_key = self.master_key();
        let secrets = store.list();
        let mut reencrypted = Vec::with_capacity(secrets.len());
        for s in secrets {
            let plaintext = decrypt_with_passphrase(&s.encrypted_value, &old_key)
                .context("failed to decrypt secret during key rotation")?;
            let encrypted = encrypt_with_passphrase(&plaintext, &new_master_key)
                .context("failed to re-encrypt secret")?;
            reencrypted.push(Secret {
                id: s.id,
                name: s.name,
                provider: s.provider,
                encrypted_value: encrypted,
            });
        }
        for s in reencrypted {
            store.upsert(s);
        }
        drop(store);
        let mut key = self.master_key.lock();
        key.clear();
        key.extend_from_slice(&new_master_key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::InMemorySecretStore;

    #[test]
    fn test_roundtrip() {
        let manager = SecretManager::new(InMemorySecretStore::new(), b"master-key".to_vec());
        let id = manager.set("openai", "llm", "sk-abc").unwrap();
        let (_, value) = manager.get_decrypted(&id).unwrap().unwrap();
        assert_eq!(value, "sk-abc");
    }

    #[test]
    fn test_remove() {
        let manager = SecretManager::new(InMemorySecretStore::new(), b"master-key".to_vec());
        let id = manager.set("openai", "llm", "sk-abc").unwrap();
        manager.remove(&id).unwrap();
        assert!(manager.get_decrypted(&id).unwrap().is_none());
    }

    #[test]
    fn test_rotate_key() {
        let manager = SecretManager::new(InMemorySecretStore::new(), b"old-key".to_vec());
        let id = manager.set("openai", "llm", "sk-abc").unwrap();
        manager.rotate_key(b"new-key".to_vec()).unwrap();
        let (_, value) = manager.get_decrypted(&id).unwrap().unwrap();
        assert_eq!(value, "sk-abc");
    }
}
