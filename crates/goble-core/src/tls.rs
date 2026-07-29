use anyhow::Result;

pub use crate::identity::Identity;
use crate::identity::{ClusterCa, ClusterIdentity, ClusterRole};

/// Self-contained CA + server + client certificate generator for mTLS pairing.
#[derive(Debug, Clone, Copy)]
pub struct CertGenerator;

impl CertGenerator {
    /// Generate an ephemeral cluster CA certificate.
    pub fn generate_ca() -> Result<Identity> {
        let ca = ClusterCa::generate_new("goble-ephemeral")?;
        Ok(ca.identity)
    }

    /// Generate a worker (server) certificate signed by the provided CA.
    pub fn generate_server(ca: &Identity, san_dns: &str) -> Result<Identity> {
        let ca = ClusterCa::from_pem(
            ca.cert_pem.clone(),
            ca.key_pem.clone(),
            Default::default(),
        )?;
        ca.sign_worker(san_dns, 365)
    }

    /// Generate a desktop (client) certificate signed by the provided CA.
    pub fn generate_client(ca: &Identity, worker_id: &str) -> Result<Identity> {
        let ca = ClusterCa::from_pem(
            ca.cert_pem.clone(),
            ca.key_pem.clone(),
            Default::default(),
        )?;
        ca.sign_device(worker_id, ClusterRole::Admin, 365)
    }
}

/// Pairing bundle exchanged (securely) between desktop and worker during setup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairingBundle {
    pub ca_cert_pem: String,
    pub ca_key_pem: Option<String>,
    pub worker_cert_pem: String,
    pub worker_key_pem: String,
    pub desktop_cert_pem: String,
    pub desktop_key_pem: String,
    pub pairing_code_hash: String,
}

impl PairingBundle {
    /// Build a pairing bundle for a worker from the active cluster identity.
    pub fn for_worker(
        cluster: &ClusterIdentity,
        san_dns: &str,
        pairing_code_hash: impl Into<String>,
    ) -> Result<Self> {
        let worker = cluster.ca.sign_worker(san_dns, 365)?;
        let desktop = cluster.device.clone();
        Ok(Self {
            ca_cert_pem: cluster.ca.identity.cert_pem.clone(),
            ca_key_pem: Some(cluster.ca.identity.key_pem.clone()),
            worker_cert_pem: worker.cert_pem,
            worker_key_pem: worker.key_pem,
            desktop_cert_pem: desktop.cert_pem,
            desktop_key_pem: desktop.key_pem,
            pairing_code_hash: pairing_code_hash.into(),
        })
    }
}

impl PairingBundle {
    /// Build a rustls server config that requires a client cert signed by the CA and
    /// carrying an operator role (Owner, Admin, or Operator).
    pub fn server_config(&self) -> Result<rustls::ServerConfig> {
        let ca = ClusterCa::from_ca_cert_pem(&self.ca_cert_pem)?;
        let worker = Identity::from_pem(
            self.worker_cert_pem.clone(),
            self.worker_key_pem.clone(),
        )?;
        ca.server_config(
            &worker,
            vec![
                ClusterRole::Owner,
                ClusterRole::Admin,
                ClusterRole::Operator,
            ],
        )
    }

    /// Build a rustls client config that verifies the server has the Worker role and
    /// presents the desktop client certificate.
    pub fn client_config(&self) -> Result<rustls::ClientConfig> {
        let ca = ClusterCa::from_ca_cert_pem(&self.ca_cert_pem)?;
        let desktop = Identity::from_pem(
            self.desktop_cert_pem.clone(),
            self.desktop_key_pem.clone(),
        )?;
        ca.client_config(&desktop, ClusterRole::Worker)
    }
}

/// Build a rustls server config that requires a client cert signed by the CA and carrying
/// an operator role.
pub fn mtls_server_config(
    server_identity: &Identity,
    ca: &Identity,
) -> Result<rustls::ServerConfig> {
    let ca = ClusterCa::from_ca_cert_pem(&ca.cert_pem)?;
    ca.server_config(
        server_identity,
        vec![
            ClusterRole::Owner,
            ClusterRole::Admin,
            ClusterRole::Operator,
        ],
    )
}

/// Build a rustls client config that verifies the server has the Worker role and presents
/// a client certificate signed by the CA.
pub fn mtls_client_config(
    client_identity: &Identity,
    ca: &Identity,
) -> Result<rustls::ClientConfig> {
    let ca = ClusterCa::from_ca_cert_pem(&ca.cert_pem)?;
    ca.client_config(client_identity, ClusterRole::Worker)
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
            ca_key_pem: None,
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
