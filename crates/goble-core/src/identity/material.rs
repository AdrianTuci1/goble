use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::role::{extract_role, extract_serial, ClusterRole};

/// An issued certificate plus its private key in PEM form.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
    pub(crate) serial: String,
    pub(crate) role: ClusterRole,
}

impl Identity {
    /// Build an identity from an end-entity certificate and key. The certificate must contain
    /// the Goble role extension.
    pub fn from_pem(cert_pem: String, key_pem: String) -> Result<Self> {
        let serial = extract_serial(&cert_pem)?;
        let role = extract_role(&cert_pem)?;
        Ok(Self {
            cert_pem,
            key_pem,
            serial,
            role,
        })
    }

    /// Build an identity for a CA certificate. The CA does not need a role extension.
    pub fn from_ca_pem(cert_pem: String, key_pem: String) -> Result<Self> {
        let serial = extract_serial(&cert_pem)?;
        Ok(Self {
            cert_pem,
            key_pem,
            serial,
            role: ClusterRole::Owner,
        })
    }

    /// Parse the certificate chain as DER for rustls.
    pub fn cert_chain(&self) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
        let mut bytes = self.cert_pem.as_bytes();
        let iter = rustls_pemfile::certs(&mut bytes);
        let mut chain = Vec::new();
        for cert in iter {
            chain.push(cert?.clone());
        }
        if chain.is_empty() {
            anyhow::bail!("no certificates found in PEM");
        }
        Ok(chain)
    }

    /// Parse the private key as DER for rustls.
    pub fn private_key(&self) -> Result<rustls::pki_types::PrivateKeyDer<'static>> {
        let mut bytes = self.key_pem.as_bytes();
        if let Some(key) = rustls_pemfile::private_key(&mut bytes)? {
            Ok(key.clone_key())
        } else {
            anyhow::bail!("no private key found in PEM");
        }
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn role(&self) -> ClusterRole {
        self.role
    }

    /// Build a root cert store containing this certificate.
    pub fn root_cert_store(&self) -> Result<Arc<rustls::RootCertStore>> {
        let mut store = rustls::RootCertStore::empty();
        for cert in self.cert_chain()? {
            store
                .add(cert)
                .context("failed to add cert to root store")?;
        }
        Ok(Arc::new(store))
    }
}
