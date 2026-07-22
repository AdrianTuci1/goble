use std::path::PathBuf;

use goble_core::vault::CredentialVault;

/// Encrypted credential vault persisted to disk.
#[derive(Debug, Clone)]
pub struct FileVault {
    path: PathBuf,
    vault: CredentialVault,
}

impl FileVault {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            vault: CredentialVault::new(),
        }
    }

    pub fn load(&mut self) -> anyhow::Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let bytes = std::fs::read(&self.path)?;
        self.vault = CredentialVault::from_bytes(&bytes)?;
        Ok(())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let bytes = self.vault.to_bytes()?;
        std::fs::create_dir_all(self.path.parent().unwrap_or(std::path::Path::new(".")))?;
        std::fs::write(&self.path, bytes)?;
        Ok(())
    }

    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: &[u8],
        passphrase: &[u8],
    ) -> anyhow::Result<()> {
        self.vault.set(key, value, passphrase)?;
        self.save()
    }

    pub fn get(&self, key: &str, passphrase: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        self.vault.get(key, passphrase)
    }

    pub fn remove(&mut self, key: &str) -> anyhow::Result<()> {
        self.vault.remove(key)?;
        self.save()
    }

    pub fn keys(&self) -> Vec<String> {
        self.vault.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_file_vault_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vault.json");
        let mut vault = FileVault::new(path.clone());
        vault.set("x", b"y", b"p").unwrap();
        drop(vault);

        let mut loaded = FileVault::new(path);
        loaded.load().unwrap();
        assert_eq!(loaded.keys(), vec!["x"]);
        assert_eq!(loaded.get("x", b"p").unwrap().unwrap(), b"y");
    }

    #[test]
    fn test_file_vault_wrong_passphrase() {
        let tmp = TempDir::new().unwrap();
        let mut vault = FileVault::new(tmp.path().join("vault.json"));
        vault.set("x", b"y", b"p").unwrap();
        assert!(vault.get("x", b"wrong").is_err());
    }
}
