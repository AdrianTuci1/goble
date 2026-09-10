use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::crypto::{decrypt_with_passphrase, encrypt_with_passphrase};
use crate::identity::ClusterRole;

/// Encrypted container that stores an arbitrary plaintext blob under a user
/// passphrase. The format is versioned so future migrations can be detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedWallet {
    pub version: u32,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl EncryptedWallet {
    pub const CURRENT_VERSION: u32 = 1;

    /// Encrypt `plaintext` with `passphrase`. The resulting wallet can be
    /// serialized to disk and later decrypted with `open`.
    pub fn seal(plaintext: &[u8], passphrase: &[u8]) -> Result<Self> {
        if passphrase.is_empty() {
            anyhow::bail!("wallet passphrase cannot be empty");
        }
        let blob = encrypt_with_passphrase(plaintext, passphrase)?;
        if blob.len() < 16 + 12 {
            anyhow::bail!("unexpected short ciphertext from encrypt_with_passphrase");
        }
        let salt = blob[..16].to_vec();
        let nonce = blob[16..16 + 12].to_vec();
        let ciphertext = blob[16 + 12..].to_vec();
        Ok(Self {
            version: Self::CURRENT_VERSION,
            salt,
            nonce,
            ciphertext,
        })
    }

    /// Decrypt the wallet using `passphrase`.
    pub fn open(&self, passphrase: &[u8]) -> Result<Vec<u8>> {
        if self.version != Self::CURRENT_VERSION {
            anyhow::bail!("unsupported encrypted wallet version {}", self.version);
        }
        let mut blob =
            Vec::with_capacity(self.salt.len() + self.nonce.len() + self.ciphertext.len());
        blob.extend_from_slice(&self.salt);
        blob.extend_from_slice(&self.nonce);
        blob.extend_from_slice(&self.ciphertext);
        decrypt_with_passphrase(&blob, passphrase).context("failed to decrypt wallet")
    }
}

/// A known device entry stored inside the identity wallet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub device_id: String,
    pub name: String,
    pub role: ClusterRole,
    pub cert_pem: String,
}

/// A known worker entry stored inside the identity wallet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerEntry {
    pub worker_id: String,
    pub name: String,
    pub url: Option<String>,
}

/// The portable cluster identity. This is the only data a new device needs to
/// join a cluster, operate existing workers, and restore configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityWallet {
    pub version: u32,
    pub cluster_key_base64: String,
    pub cluster_name: String,
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub revoked_serials: Vec<String>,
    pub devices: Vec<DeviceEntry>,
    pub workers: Vec<WorkerEntry>,
}

impl IdentityWallet {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new(
        cluster_key_base64: impl Into<String>,
        cluster_name: impl Into<String>,
        ca_cert_pem: impl Into<String>,
        ca_key_pem: impl Into<String>,
    ) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            cluster_key_base64: cluster_key_base64.into(),
            cluster_name: cluster_name.into(),
            ca_cert_pem: ca_cert_pem.into(),
            ca_key_pem: ca_key_pem.into(),
            revoked_serials: Vec::new(),
            devices: Vec::new(),
            workers: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize identity wallet")
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("failed to deserialize identity wallet")
    }

    /// Seal the wallet with a user passphrase, producing an encrypted blob that
    /// can be exported to a file.
    pub fn seal(&self, passphrase: &[u8]) -> Result<EncryptedWallet> {
        let plaintext = self.to_json()?;
        EncryptedWallet::seal(plaintext.as_bytes(), passphrase)
    }

    /// Open a sealed wallet from a file.
    pub fn open(wallet: &EncryptedWallet, passphrase: &[u8]) -> Result<Self> {
        let plaintext = wallet.open(passphrase)?;
        let json = String::from_utf8(plaintext).context("wallet plaintext is not utf-8")?;
        Self::from_json(&json)
    }

    pub fn add_device(&mut self, entry: DeviceEntry) {
        self.devices.retain(|d| d.device_id != entry.device_id);
        self.devices.push(entry);
    }

    pub fn remove_device(&mut self, device_id: &str) {
        self.devices.retain(|d| d.device_id != device_id);
    }

    pub fn add_worker(&mut self, entry: WorkerEntry) {
        self.workers.retain(|w| w.worker_id != entry.worker_id);
        self.workers.push(entry);
    }

    pub fn remove_worker(&mut self, worker_id: &str) {
        self.workers.retain(|w| w.worker_id != worker_id);
    }

    pub fn revoke_serial(&mut self, serial: impl Into<String>) {
        let serial = serial.into();
        if !self.revoked_serials.contains(&serial) {
            self.revoked_serials.push(serial);
        }
    }

    pub fn to_cluster_identity(
        &self,
        device_id: &str,
        role: ClusterRole,
    ) -> Result<crate::cluster_key::ClusterIdentity> {
        let key = crate::cluster_key::ClusterKey::from_base64(&self.cluster_key_base64)
            .context("invalid cluster key in identity wallet")?;
        crate::cluster_key::ClusterIdentity::from_key(key, &self.cluster_name, device_id, role)
    }
}

impl From<&crate::cluster_key::ClusterIdentity> for IdentityWallet {
    fn from(identity: &crate::cluster_key::ClusterIdentity) -> Self {
        let mut wallet = IdentityWallet::new(
            identity.key.to_base64(),
            identity.cluster_name.clone(),
            identity.ca.identity.cert_pem.clone(),
            identity.ca.identity.key_pem.clone(),
        );
        wallet.add_device(crate::encrypted_wallet::DeviceEntry {
            device_id: identity.device.serial().to_string(),
            name: "owner-device".to_string(),
            role: identity.device.role(),
            cert_pem: identity.device.cert_pem.clone(),
        });
        wallet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = b"cluster identity data";
        let wallet = EncryptedWallet::seal(data, b"password123").unwrap();
        let opened = wallet.open(b"password123").unwrap();
        assert_eq!(opened, data.as_slice());
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let data = b"cluster identity data";
        let wallet = EncryptedWallet::seal(data, b"password123").unwrap();
        assert!(wallet.open(b"wrong").is_err());
    }

    #[test]
    fn test_empty_passphrase_rejected() {
        use crate::vault::CredentialVault;
        let mut vault = CredentialVault::new();
        let err = vault.set("x", b"y", b"").unwrap_err().to_string();
        assert!(err.contains("passphrase cannot be empty"), "{err}");

        let data = b"cluster identity data";
        let err = EncryptedWallet::seal(data, b"").unwrap_err().to_string();
        assert!(err.contains("passphrase cannot be empty"), "{err}");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let wallet = EncryptedWallet::seal(b"secret", b"pass").unwrap();
        let json = serde_json::to_string(&wallet).unwrap();
        let loaded: EncryptedWallet = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.open(b"pass").unwrap(), b"secret");
    }

    #[test]
    fn test_identity_wallet_roundtrip_in_snapshot() {
        let mut identity = IdentityWallet::new("c3Vkaw==", "test-cluster", "ca-pem", "ca-key-pem");
        identity.add_device(DeviceEntry {
            device_id: "dev-1".to_string(),
            name: "laptop".to_string(),
            role: ClusterRole::Owner,
            cert_pem: "cert".to_string(),
        });
        identity.add_worker(WorkerEntry {
            worker_id: "worker-1".to_string(),
            name: "remote-1".to_string(),
            url: Some("https://1.2.3.4".to_string()),
        });

        let sealed = identity.seal(b"secret-pass").unwrap();
        let restored = IdentityWallet::open(&sealed, b"secret-pass").unwrap();
        assert_eq!(restored, identity);
    }
}
