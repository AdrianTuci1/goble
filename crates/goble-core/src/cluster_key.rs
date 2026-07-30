use std::fmt;

use anyhow::{Context, Result};
use base64::Engine;
use rcgen::KeyPair;
use ring::hkdf::{Salt, HKDF_SHA256};
use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::identity::{ClusterCa, ClusterRole, Identity};

/// Single secret that defines a Goble cluster. A 32-byte seed is used to
/// deterministically derive the cluster root CA and a symmetric backup key.
/// Users can export/import this seed as a single base64 string to move between
/// devices without passwords or accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterKey([u8; 32]);

impl ClusterKey {
    /// Generate a fresh random cluster key.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::fill(&mut seed);
        Self(seed)
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self(seed)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parse the key from a URL-safe, unpadded base64 string.
    pub fn from_base64(s: &str) -> Result<Self> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.trim())
            .context("invalid cluster key encoding")?;
        if bytes.len() != 32 {
            anyhow::bail!("cluster key must be 32 bytes, got {}", bytes.len());
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        Ok(Self(seed))
    }

    /// Export as a URL-safe, unpadded base64 string.
    pub fn to_base64(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&self.0)
    }

    /// Derive a deterministic 32-byte symmetric key used for encrypted backup bundles.
    pub fn derive_backup_key(&self) -> [u8; 32] {
        let salt = Salt::new(HKDF_SHA256, b"goble-cluster-key");
        let prk = salt.extract(&self.0);
        let mut okm = [0u8; 32];
        let _ = prk.expand(&[b"backup"], ring::hkdf::HKDF_SHA256)
            .expect("32 bytes is within HKDF limit")
            .fill(&mut okm);
        okm
    }

    /// Derive a deterministic 32-byte key suitable for vault encryption.
    pub fn derive_vault_key(&self) -> [u8; 32] {
        let salt = Salt::new(HKDF_SHA256, b"goble-cluster-key");
        let prk = salt.extract(&self.0);
        let mut okm = [0u8; 32];
        let _ = prk.expand(&[b"vault"], ring::hkdf::HKDF_SHA256)
            .expect("32 bytes is within HKDF limit")
            .fill(&mut okm);
        okm
    }

    /// Deterministically derive the cluster root CA Ed25519 key pair from this seed.
    pub fn derive_ca_keypair(&self) -> KeyPair {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let signing_key = Ed25519KeyPair::from_seed_unchecked(&self.0)
            .expect("32 bytes seed is a valid Ed25519 seed");
        let public_key: &[u8; 32] = signing_key.public_key().as_ref().try_into().expect("Ed25519 public key is 32 bytes");
        let pkcs8 = build_ed25519_pkcs8_v2(&self.0, public_key);
        let pkcs8_der = rustls::pki_types::PrivatePkcs8KeyDer::from(pkcs8);
        rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&pkcs8_der, &rcgen::PKCS_ED25519)
            .expect("deterministic PKCS#8 v2 is valid for rcgen")
    }

    /// Build the cluster root CA from this key.
    pub fn to_ca(&self, cluster_name: impl AsRef<str>) -> Result<ClusterCa> {
        ClusterCa::from_key(self, cluster_name)
    }
}

impl fmt::Display for ClusterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base64())
    }
}

impl std::str::FromStr for ClusterKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_base64(s)
    }
}

/// Encrypted cluster backup bundle. The plaintext is encrypted with the backup
/// key derived from the cluster key, so importing a cluster key is sufficient to
/// decrypt it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterBackup {
    /// Version of the backup format.
    pub version: u32,
    /// Base64-encoded ciphertext produced by `crypto::encrypt`.
    pub ciphertext: String,
}

/// Plaintext payload inside an encrypted backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupPayload {
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub revoked_serials: Vec<String>,
    pub peer_addresses: Vec<String>,
}

impl ClusterBackup {
    /// Export a backup from a CA that was created from this cluster key. The
    /// backup includes the CA certificate and key so that it can be restored on
    /// another device or recovered from a worker.
    pub fn from_ca(cluster_key: &ClusterKey, ca: &ClusterCa) -> Result<Self> {
        let store = ca.store.read().unwrap();
        let payload = BackupPayload {
            ca_cert_pem: ca.identity.cert_pem.clone(),
            ca_key_pem: ca.identity.key_pem.clone(),
            revoked_serials: store.revoked.iter().cloned().collect(),
            peer_addresses: Vec::new(),
        };
        let plaintext = serde_json::to_vec(&payload).context("failed to serialize backup")?;
        let encrypted = crypto::encrypt(&plaintext, &cluster_key.derive_backup_key())?;
        Ok(Self {
            version: 1,
            ciphertext: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&encrypted),
        })
    }

    /// Decrypt the backup and reconstruct the CA. The returned CA is ready to
    /// issue new certificates.
    pub fn restore_ca(&self, cluster_key: &ClusterKey) -> Result<ClusterCa> {
        if self.version != 1 {
            anyhow::bail!("unsupported backup version {}", self.version);
        }
        let encrypted = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&self.ciphertext)
            .context("invalid backup encoding")?;
        let plaintext = crypto::decrypt(&encrypted, &cluster_key.derive_backup_key())?;
        let payload: BackupPayload =
            serde_json::from_slice(&plaintext).context("invalid backup payload")?;
        let ca = ClusterCa::from_pem(payload.ca_cert_pem, payload.ca_key_pem, Default::default())?;
        for serial in payload.revoked_serials {
            let _ = ca.revoke(&serial);
        }
        Ok(ca)
    }
}

/// Serializable view of a cluster identity for persistence and transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterIdentitySnapshot {
    pub version: u32,
    pub cluster_name: String,
    pub key: String,
    pub device_cert_pem: String,
    pub device_key_pem: String,
}

/// Convenience wrapper that holds a cluster key, the derived CA, and the local
/// device identity.
#[derive(Debug, Clone)]
pub struct ClusterIdentity {
    pub cluster_name: String,
    pub key: ClusterKey,
    pub ca: ClusterCa,
    pub device: Identity,
}

impl ClusterIdentity {
    /// Create a new cluster and issue a device certificate for this device.
    pub fn generate(cluster_name: impl Into<String>, device_id: &str, role: ClusterRole) -> Result<Self> {
        let cluster_name = cluster_name.into();
        let key = ClusterKey::generate();
        let ca = key.to_ca(&cluster_name)?;
        let device = ca.sign_device(device_id, role, 365)?;
        Ok(Self { cluster_name, key, ca, device })
    }

    /// Restore a cluster from an exported key and issue a device certificate.
    /// This is used when a user imports a cluster key on a new device.
    pub fn from_key(
        cluster_key: ClusterKey,
        cluster_name: impl Into<String>,
        device_id: &str,
        role: ClusterRole,
    ) -> Result<Self> {
        let cluster_name = cluster_name.into();
        let ca = cluster_key.to_ca(&cluster_name)?;
        let device = ca.sign_device(device_id, role, 365)?;
        Ok(Self { cluster_name, key: cluster_key, ca, device })
    }

    /// Restore a cluster from an encrypted backup and issue a device certificate.
    pub fn from_backup(
        backup: &ClusterBackup,
        cluster_key: ClusterKey,
        device_id: &str,
        role: ClusterRole,
    ) -> Result<Self> {
        let ca = backup.restore_ca(&cluster_key)?;
        let cluster_name = "restored-cluster".to_string();
        let device = ca.sign_device(device_id, role, 365)?;
        Ok(Self { cluster_name, key: cluster_key, ca, device })
    }

    /// Serialize the identity to a storable snapshot.
    pub fn to_snapshot(&self) -> ClusterIdentitySnapshot {
        ClusterIdentitySnapshot {
            version: 1,
            cluster_name: self.cluster_name.clone(),
            key: self.key.to_base64(),
            device_cert_pem: self.device.cert_pem.clone(),
            device_key_pem: self.device.key_pem.clone(),
        }
    }

    /// Restore an identity from a snapshot, reconstructing the deterministic CA.
    pub fn from_snapshot(snapshot: ClusterIdentitySnapshot) -> Result<Self> {
        let key = ClusterKey::from_base64(&snapshot.key)?;
        let ca = key.to_ca(&snapshot.cluster_name)?;
        let device = Identity::from_pem(snapshot.device_cert_pem, snapshot.device_key_pem)?;
        Ok(Self {
            cluster_name: snapshot.cluster_name,
            key,
            ca,
            device,
        })
    }

    /// Export the cluster key as a base64 string.
    pub fn export_key(&self) -> String {
        self.key.to_base64()
    }

    /// Export an encrypted backup bundle containing the CA certificate and key.
    pub fn export_backup(&self) -> Result<ClusterBackup> {
        ClusterBackup::from_ca(&self.key, &self.ca)
    }
}


/// Build a PKCS#8 v2 Ed25519 document from a seed and public key using the
/// fixed DER template from ring. This is deterministic and serializable by rcgen.
fn build_ed25519_pkcs8_v2(seed: &[u8; 32], public_key: &[u8; 32]) -> Vec<u8> {
    const PREFIX: &[u8] = &[
        0x30, 0x51, // SEQUENCE, length 81
        0x02, 0x01, 0x01, // INTEGER version 1
        0x30, 0x05, // SEQUENCE algorithm identifier, length 5
        0x06, 0x03, 0x2b, 0x65, 0x70, // OID Ed25519 (1.3.101.112)
        0x04, 0x22, // OCTET STRING, length 34
        0x04, 0x20, // OCTET STRING, length 32 (seed)
    ];
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(seed);
    // [1] BIT STRING for the public key, length 33, unused bits = 0.
    out.extend_from_slice(&[0x81, 0x21, 0x00]);
    out.extend_from_slice(public_key);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_key_roundtrip() {
        let key = ClusterKey::generate();
        let encoded = key.to_base64();
        let decoded = ClusterKey::from_base64(&encoded).unwrap();
        assert_eq!(key, decoded);
        assert_eq!(encoded, decoded.to_base64());
    }

    #[test]
    fn test_cluster_key_deterministic_ca() {
        let key = ClusterKey::generate();
        let ca1 = key.to_ca("cluster-a").unwrap();
        let ca2 = key.to_ca("cluster-a").unwrap();
        assert_eq!(ca1.identity.cert_pem, ca2.identity.cert_pem);
        assert_eq!(ca1.identity.key_pem, ca2.identity.key_pem);
    }

    #[test]
    fn test_cluster_ca_differs_by_name() {
        let key = ClusterKey::generate();
        let ca1 = key.to_ca("cluster-a").unwrap();
        let ca2 = key.to_ca("cluster-b").unwrap();
        assert_ne!(ca1.identity.cert_pem, ca2.identity.cert_pem);
    }

    #[test]
    fn test_backup_restore_roundtrip() {
        let key = ClusterKey::generate();
        let ca = key.to_ca("cluster-a").unwrap();
        let device = ca.sign_device("desktop-1", ClusterRole::Admin, 365).unwrap();
        ca.revoke(device.serial()).unwrap();

        let backup = ClusterBackup::from_ca(&key, &ca).unwrap();
        let restored_ca = backup.restore_ca(&key).unwrap();
        assert_eq!(restored_ca.identity.cert_pem, ca.identity.cert_pem);
        assert_eq!(restored_ca.identity.key_pem, ca.identity.key_pem);
        assert!(restored_ca.store.read().unwrap().is_revoked(device.serial()));
    }

    #[test]
    fn test_cluster_identity_from_key_restored() {
        let cluster = ClusterIdentity::generate("cluster-a", "desktop-1", ClusterRole::Admin).unwrap();
        let restored = ClusterIdentity::from_key(
            cluster.key.clone(),
            "cluster-a",
            "desktop-2",
            ClusterRole::Admin,
        )
        .unwrap();
        assert_eq!(restored.ca.identity.cert_pem, cluster.ca.identity.cert_pem);
        assert_eq!(restored.key, cluster.key);
        assert_ne!(restored.device.serial(), cluster.device.serial());
    }

    #[test]
    fn test_wrong_cluster_key_fails_to_decrypt() {
        let key = ClusterKey::generate();
        let wrong_key = ClusterKey::generate();
        let ca = key.to_ca("cluster-a").unwrap();
        let backup = ClusterBackup::from_ca(&key, &ca).unwrap();
        assert!(backup.restore_ca(&wrong_key).is_err());
    }
}
