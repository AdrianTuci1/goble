use anyhow::Result;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

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
}

/// Pairing bundle exchanged (securely) between desktop and worker during setup.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairingBundle {
    pub ca_cert_pem: String,
    pub worker_cert_pem: String,
    pub worker_key_pem: String,
    pub pairing_code_hash: String,
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
        let bundle = PairingBundle {
            ca_cert_pem: ca.cert_pem,
            worker_cert_pem: server.cert_pem,
            worker_key_pem: server.key_pem,
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
}
