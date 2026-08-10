use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::crypto::{decrypt_with_passphrase, encrypt_with_passphrase};

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
    fn test_empty_passphrase_roundtrip() {
        let data = b"";
        let wallet = EncryptedWallet::seal(data, b"").unwrap();
        let opened = wallet.open(b"").unwrap();
        assert!(opened.is_empty());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let wallet = EncryptedWallet::seal(b"secret", b"pass").unwrap();
        let json = serde_json::to_string(&wallet).unwrap();
        let loaded: EncryptedWallet = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.open(b"pass").unwrap(), b"secret");
    }
}
