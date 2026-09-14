use std::collections::{HashMap, HashSet};

use anyhow::Result;
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::ca::load_ed25519_key;
use super::crl::SignedCrl;
use super::material::Identity;
use super::role::extract_serial;

/// In-memory store of certificates known to a cluster participant plus revocation state.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CertificateStore {
    /// Active certificates by PEM-encoded serial number (hex, lowercase).
    pub active: HashMap<String, String>,
    /// Revoked serial numbers (hex, lowercase).
    pub revoked: HashSet<String>,
    /// Monotonic CRL version.
    pub crl_version: u64,
    /// The last signed CRL document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crl: Option<SignedCrl>,
}

impl CertificateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, cert_pem: String) -> Result<()> {
        let serial = extract_serial(&cert_pem)?;
        if self.revoked.contains(&serial) {
            anyhow::bail!("certificate {} is revoked", serial);
        }
        self.active.insert(serial, cert_pem);
        Ok(())
    }

    pub fn add_identity(&mut self, identity: &Identity) -> Result<()> {
        self.add(identity.cert_pem.clone())
    }

    pub fn add_ca(&mut self, cert_pem: String) -> Result<()> {
        let serial = extract_serial(&cert_pem)?;
        self.active.insert(serial, cert_pem);
        Ok(())
    }

    pub fn remove(&mut self, serial: &str) {
        self.active.remove(serial);
    }

    pub fn revoke(&mut self, serial: &str) -> bool {
        let serial = serial.to_lowercase();
        self.remove(&serial);
        self.revoked.insert(serial)
    }

    pub fn is_revoked(&self, serial: &str) -> bool {
        self.revoked.contains(&serial.to_lowercase())
    }

    pub fn is_active(&self, serial: &str) -> bool {
        self.active.contains_key(&serial.to_lowercase())
    }

    pub fn cert_pem(&self, serial: &str) -> Option<&str> {
        self.active.get(&serial.to_lowercase()).map(|s| s.as_str())
    }

    /// Return a signed CRL document using the cluster CA private key.
    pub fn sign_crl(&self, ca_key_pem: &str) -> Result<SignedCrl> {
        let key_pair = load_ed25519_key(ca_key_pem)?;
        let mut crl = SignedCrl {
            version: self.crl_version,
            issued_at: Utc::now(),
            revoked_serials: self.revoked.iter().cloned().collect(),
            signature: String::new(),
        };
        crl.revoked_serials.sort();
        let sig = key_pair.sign(crl.canonical_bytes().as_slice());
        crl.signature = base64::engine::general_purpose::STANDARD.encode(sig.as_ref());
        Ok(crl)
    }

    /// Apply a newer CRL if its version is greater and its signature is valid.
    pub fn apply_crl(&mut self, crl: SignedCrl, ca_cert_pem: &str) -> Result<bool> {
        if crl.version <= self.crl_version {
            return Ok(false);
        }
        crl.verify(ca_cert_pem)?;
        self.crl_version = crl.version;
        self.crl = Some(crl.clone());
        for serial in crl.revoked_serials {
            self.revoked.insert(serial);
        }
        // Purge any active certs that are now revoked.
        let revoked = self.revoked.clone();
        self.active.retain(|serial, _| !revoked.contains(serial));
        Ok(true)
    }
}
