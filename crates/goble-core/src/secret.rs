use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secret {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub encrypted_value: Vec<u8>,
}

impl Secret {
    pub fn new(
        name: impl Into<String>,
        provider: impl Into<String>,
        encrypted_value: Vec<u8>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            provider: provider.into(),
            encrypted_value,
        }
    }
}

pub trait SecretStore: Send + Sync {
    fn list(&self) -> Vec<Secret>;
    fn get(&self, id: &str) -> Option<Secret>;
    fn insert(&mut self, secret: Secret) -> anyhow::Result<()>;
    fn remove(&mut self, id: &str) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemorySecretStore {
    secrets: Vec<Secret>,
}

impl InMemorySecretStore {
    pub fn new() -> Self {
        Self {
            secrets: Vec::new(),
        }
    }
}

impl SecretStore for InMemorySecretStore {
    fn list(&self) -> Vec<Secret> {
        self.secrets.clone()
    }

    fn get(&self, id: &str) -> Option<Secret> {
        self.secrets.iter().find(|s| s.id == id).cloned()
    }

    fn insert(&mut self, secret: Secret) -> anyhow::Result<()> {
        if self.secrets.iter().any(|s| s.id == secret.id) {
            anyhow::bail!("secret already exists");
        }
        self.secrets.push(secret);
        Ok(())
    }

    fn remove(&mut self, id: &str) -> anyhow::Result<()> {
        let len = self.secrets.len();
        self.secrets.retain(|s| s.id != id);
        if self.secrets.len() == len {
            anyhow::bail!("secret not found");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store() {
        let mut store = InMemorySecretStore::new();
        let secret = Secret::new("openai", "llm", vec![1, 2, 3]);
        let id = secret.id.clone();
        store.insert(secret.clone()).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.get(&id).unwrap().name, "openai");
        store.remove(&id).unwrap();
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_duplicate_insert_fails() {
        let mut store = InMemorySecretStore::new();
        let secret = Secret::new("openai", "llm", vec![1, 2, 3]);
        store.insert(secret.clone()).unwrap();
        assert!(store.insert(secret).is_err());
    }

    #[test]
    fn test_remove_missing_fails() {
        let mut store = InMemorySecretStore::new();
        assert!(store.remove("missing").is_err());
    }
}
