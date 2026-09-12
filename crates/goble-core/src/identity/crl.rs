use anyhow::{Context, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use x509_parser::prelude::*;

use super::pem::pem_to_der;

/// A signed CRL-like document that lists revoked certificate serials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCrl {
    pub version: u64,
    pub issued_at: DateTime<Utc>,
    pub revoked_serials: Vec<String>,
    pub signature: String,
}

impl SignedCrl {
    pub(super) fn canonical_bytes(&self) -> Vec<u8> {
        let mut copy = self.clone();
        copy.signature.clear();
        serde_json::to_vec(&copy).expect("CRL serializes to JSON")
    }

    pub fn verify(&self, ca_cert_pem: &str) -> Result<()> {
        let der = pem_to_der(ca_cert_pem)?;
        let (_, x509) = X509Certificate::from_der(&der)
            .map_err(|e| anyhow::anyhow!("failed to parse CA certificate: {e}"))?;

        let public_key_der = x509.subject_pki.subject_public_key.as_ref();
        let public_key = UnparsedPublicKey::new(&ED25519, public_key_der);

        let sig = base64::engine::general_purpose::STANDARD
            .decode(&self.signature)
            .context("CRL signature is not valid base64")?;
        public_key
            .verify(&self.canonical_bytes(), &sig)
            .map_err(|_| anyhow::anyhow!("CRL signature is invalid"))?;
        Ok(())
    }
}
