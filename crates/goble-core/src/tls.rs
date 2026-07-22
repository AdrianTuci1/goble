use std::sync::Arc;

use anyhow::{Context, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::RootCertStore;

/// Holds a generated certificate plus its private key in PEM form.
#[derive(Debug, Clone)]
pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
    pub(crate) params: CertificateParams,
}

/// A self-contained CA + server + client certificate generator for mTLS pairing.
pub struct CertGenerator;

impl CertGenerator {
    /// Generate an ephemeral CA certificate.
    pub fn generate_ca() -> Result<Identity> {
        let mut params = Self::base_params();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params.distinguished_name = Self::dn("Goble Ephemeral CA");

        let key = KeyPair::generate()?;
        let cert = params.clone().self_signed(&key)?;
        Ok(Identity::new(cert.pem(), key.serialize_pem(), params))
    }

    /// Generate a server certificate signed by the provided CA.
    pub fn generate_server(ca: &Identity, san_dns: &str) -> Result<Identity> {
        let ca_key = KeyPair::from_pem(&ca.key_pem)?;
        let ca_cert = ca.params.clone().self_signed(&ca_key)?;

        let mut params = Self::base_params();
        params.distinguished_name = Self::dn(&format!("goblin-{}", san_dns));
        params.subject_alt_names = vec![SanType::DnsName(san_dns.try_into().unwrap())];

        let key = KeyPair::generate()?;
        let cert = params.clone().signed_by(&key, &ca_cert, &ca_key)?;
        Ok(Identity::new(cert.pem(), key.serialize_pem(), params))
    }

    /// Generate a client certificate signed by the provided CA.
    pub fn generate_client(ca: &Identity, worker_id: &str) -> Result<Identity> {
        let ca_key = KeyPair::from_pem(&ca.key_pem)?;
        let ca_cert = ca.params.clone().self_signed(&ca_key)?;

        let mut params = Self::base_params();
        params.distinguished_name = Self::dn(&format!("goble-desktop-{}", worker_id));
        params.subject_alt_names = vec![SanType::DnsName(worker_id.try_into().unwrap())];

        let key = KeyPair::generate()?;
        let cert = params.clone().signed_by(&key, &ca_cert, &ca_key)?;
        Ok(Identity::new(cert.pem(), key.serialize_pem(), params))
    }

    fn base_params() -> CertificateParams {
        let mut params = CertificateParams::default();
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + time::Duration::days(365);
        params
    }

    fn dn(cn: &str) -> DistinguishedName {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn);
        dn
    }
}

impl Identity {
    fn new(cert_pem: String, key_pem: String, params: CertificateParams) -> Self {
        Self {
            cert_pem,
            key_pem,
            params,
        }
    }

    /// Parse the certificate chain (single cert) as DER for rustls.
    pub fn cert_chain(&self) -> Result<Vec<CertificateDer<'static>>> {
        let mut bytes = self.cert_pem.as_bytes();
        let mut iter = rustls_pemfile::certs(&mut bytes);
        let mut chain = Vec::new();
        while let Some(cert) = iter.next() {
            chain.push(cert?.clone());
        }
        if chain.is_empty() {
            anyhow::bail!("no certificates found in PEM");
        }
        Ok(chain)
    }

    /// Parse the private key as DER for rustls.
    pub fn private_key(&self) -> Result<PrivateKeyDer<'static>> {
        let mut bytes = self.key_pem.as_bytes();
        if let Some(key) = rustls_pemfile::private_key(&mut bytes)? {
            Ok(key.clone_key())
        } else {
            anyhow::bail!("no private key found in PEM");
        }
    }

    /// Build a root cert store containing this CA certificate.
    pub fn root_cert_store(&self) -> Result<Arc<RootCertStore>> {
        let mut store = RootCertStore::empty();
        let certs = self.cert_chain()?;
        for cert in certs {
            store.add(cert).context("failed to add CA to root store")?;
        }
        Ok(Arc::new(store))
    }
}

/// Pairing bundle exchanged (securely) between desktop and worker during setup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairingBundle {
    pub ca_cert_pem: String,
    pub worker_cert_pem: String,
    pub worker_key_pem: String,
    pub desktop_cert_pem: String,
    pub desktop_key_pem: String,
    pub pairing_code_hash: String,
}

impl PairingBundle {
    /// Build a rustls server config that requires a client cert signed by the CA.
    pub fn server_config(&self) -> Result<rustls::ServerConfig> {
        let ca = Identity {
            cert_pem: self.ca_cert_pem.clone(),
            key_pem: String::new(),
            params: CertificateParams::default(),
        };
        let server = Identity {
            cert_pem: self.worker_cert_pem.clone(),
            key_pem: self.worker_key_pem.clone(),
            params: CertificateParams::default(),
        };
        mtls_server_config(&server, &ca)
    }

    /// Build a rustls client config that verifies the server and sends a client cert.
    pub fn client_config(&self) -> Result<rustls::ClientConfig> {
        let ca = Identity {
            cert_pem: self.ca_cert_pem.clone(),
            key_pem: String::new(),
            params: CertificateParams::default(),
        };
        let client = Identity {
            cert_pem: self.desktop_cert_pem.clone(),
            key_pem: self.desktop_key_pem.clone(),
            params: CertificateParams::default(),
        };
        mtls_client_config(&client, &ca)
    }
}

/// Build a rustls server config that requires a client cert signed by the CA.
pub fn mtls_server_config(
    server_identity: &Identity,
    ca: &Identity,
) -> Result<rustls::ServerConfig> {
    let cert_chain = server_identity.cert_chain()?;
    let private_key = server_identity.private_key()?;
    let root_store = ca.root_cert_store()?;

    let config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(
            rustls::server::WebPkiClientVerifier::builder(root_store.clone())
                .build()
                .context("failed to build client cert verifier")?,
        )
        .with_single_cert(cert_chain, private_key)
        .context("failed to build server config")?;

    Ok(config)
}

/// Build a rustls client config that verifies the server and sends a client cert.
pub fn mtls_client_config(
    client_identity: &Identity,
    ca: &Identity,
) -> Result<rustls::ClientConfig> {
    let cert_chain = client_identity.cert_chain()?;
    let private_key = client_identity.private_key()?;
    let root_store = ca.root_cert_store()?;

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_client_auth_cert(cert_chain, private_key)
        .context("failed to build client config")?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ca_server_client_roundtrip() {
        let ca = CertGenerator::generate_ca().unwrap();
        let server = CertGenerator::generate_server(&ca, "goblin.local").unwrap();
        let client = CertGenerator::generate_client(&ca, "worker-123").unwrap();

        assert!(ca.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(server.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(server.key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(client.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(client.key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn test_pairing_bundle_serialization() {
        let ca = CertGenerator::generate_ca().unwrap();
        let server = CertGenerator::generate_server(&ca, "goblin.local").unwrap();
        let desktop = CertGenerator::generate_client(&ca, "desktop-1").unwrap();
        let bundle = PairingBundle {
            ca_cert_pem: ca.cert_pem,
            worker_cert_pem: server.cert_pem,
            worker_key_pem: server.key_pem,
            desktop_cert_pem: desktop.cert_pem,
            desktop_key_pem: desktop.key_pem,
            pairing_code_hash: "hash123".to_string(),
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let decoded: PairingBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.pairing_code_hash, "hash123");
    }

    #[test]
    fn test_identity_clone_roundtrip() {
        let ca = CertGenerator::generate_ca().unwrap();
        let ca2 = ca.clone();
        let server = CertGenerator::generate_server(&ca2, "goblin.local").unwrap();
        assert!(server.cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_mtls_configs_build() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca = CertGenerator::generate_ca().unwrap();
        let server = CertGenerator::generate_server(&ca, "goblin.local").unwrap();
        let client = CertGenerator::generate_client(&ca, "worker-123").unwrap();

        let server_config = mtls_server_config(&server, &ca).unwrap();
        let client_config = mtls_client_config(&client, &ca).unwrap();

        assert_eq!(server_config.alpn_protocols.len(), 0);
        assert_eq!(client_config.alpn_protocols.len(), 0);
    }
}
