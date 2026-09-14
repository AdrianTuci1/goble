use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use chrono::Utc;
use rcgen::{CertificateParams, DnType, IsCa, KeyPair, SanType, SerialNumber};
use ring::signature::{Ed25519KeyPair, UnparsedPublicKey, ED25519};
use rustls::client::danger::ServerCertVerifier;
use x509_parser::prelude::*;

use super::certs::CertificateStore;
use super::crl::SignedCrl;
use super::material::Identity;
use super::pem::{offset_to_chrono, pem_to_der};
use super::role::{extract_role_from_cert, extract_serial, ClusterRole, RoleExtension};
use super::verifier::{ClusterClientVerifier, ClusterServerVerifier};

/// The cluster's root certificate authority and its in-memory store.
#[derive(Debug, Clone)]
pub struct ClusterCa {
    pub identity: Identity,
    pub store: Arc<RwLock<CertificateStore>>,
    ca_params: CertificateParams,
    ca_key: Option<Arc<KeyPair>>,
}

impl ClusterCa {
    /// Generate a new self-signed root CA for a cluster.
    pub fn generate_new(cluster_name: impl AsRef<str>) -> Result<Self> {
        let key_pair = KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        Self::build_from_key(key_pair, cluster_name)
    }

    /// Build a cluster CA from a deterministic cluster key.
    pub fn from_key(
        cluster_key: &crate::cluster_key::ClusterKey,
        cluster_name: impl AsRef<str>,
    ) -> Result<Self> {
        let key_pair = cluster_key.derive_ca_keypair();
        Self::build_from_key(key_pair, cluster_name)
    }

    fn build_from_key(key_pair: KeyPair, cluster_name: impl AsRef<str>) -> Result<Self> {
        let mut ca_params = Self::base_params(365 * 10); // 10 years
        ca_params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params.distinguished_name = Self::dn("Goble", cluster_name.as_ref());
        ca_params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];

        let cert = ca_params.clone().self_signed(&key_pair)?;
        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();
        let serial = extract_serial(&cert_pem)?;

        let identity = Identity {
            cert_pem: cert_pem.clone(),
            key_pem,
            serial,
            role: ClusterRole::Owner,
        };

        let mut store = CertificateStore::new();
        store.add_ca(identity.cert_pem.clone())?;

        Ok(Self {
            identity,
            store: Arc::new(RwLock::new(store)),
            ca_params,
            ca_key: Some(Arc::new(key_pair)),
        })
    }

    /// Reconstruct a CA from existing PEMs and store. If `ca_key_pem` is empty the CA is
    /// read-only and cannot issue new certificates.
    pub fn from_pem(
        ca_cert_pem: String,
        ca_key_pem: String,
        store: CertificateStore,
    ) -> Result<Self> {
        let identity = Identity::from_ca_pem(ca_cert_pem, ca_key_pem)?;
        let mut store = store;
        store.add_ca(identity.cert_pem.clone())?;
        let ca_params =
            CertificateParams::from_ca_cert_der(&pem_to_der(&identity.cert_pem)?.into())
                .map_err(|e| anyhow::anyhow!("failed to parse CA params: {e}"))?;
        let ca_key = if identity.key_pem.is_empty() {
            None
        } else {
            Some(Arc::new(
                KeyPair::from_pem(&identity.key_pem).context("failed to load CA key")?,
            ))
        };
        Ok(Self {
            identity,
            store: Arc::new(RwLock::new(store)),
            ca_params,
            ca_key,
        })
    }

    /// Reconstruct a read-only CA from its certificate PEM.
    pub fn from_ca_cert_pem(ca_cert_pem: impl AsRef<str>) -> Result<Self> {
        Self::from_pem(
            ca_cert_pem.as_ref().to_string(),
            String::new(),
            CertificateStore::new(),
        )
    }

    /// Sign a device certificate for a new desktop/mobile client.
    pub fn sign_device(&self, device_id: &str, role: ClusterRole, days: u64) -> Result<Identity> {
        if role.is_worker() {
            anyhow::bail!("use sign_worker to issue worker certificates");
        }
        self.sign_identity(device_id, role, days, false)
    }

    /// Sign a worker certificate for a VPS node.
    pub fn sign_worker(&self, worker_id: &str, days: u64) -> Result<Identity> {
        self.sign_identity(worker_id, ClusterRole::Worker, days, true)
    }

    /// Sign a worker and return a self-contained bundle for provisioning.
    pub fn sign_worker_bundle(
        &self,
        worker_id: &str,
        cluster_name: &str,
        days: u64,
    ) -> Result<crate::provision::WorkerBundle> {
        let identity = self.sign_worker(worker_id, days)?;
        Ok(crate::provision::WorkerBundle {
            worker_id: worker_id.to_string(),
            cluster_name: cluster_name.to_string(),
            cert_pem: identity.cert_pem,
            key_pem: identity.key_pem,
            ca_cert_pem: self.identity.cert_pem.clone(),
        })
    }

    fn sign_identity(
        &self,
        cn: &str,
        role: ClusterRole,
        days: u64,
        add_worker_san: bool,
    ) -> Result<Identity> {
        let ca_key = self
            .ca_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CA private key is not available"))?;
        let ca_cert = self
            .ca_params
            .clone()
            .self_signed(ca_key)
            .context("failed to reconstruct CA certificate")?;

        let mut params = Self::base_params(days);
        let name = if role.is_worker() {
            format!("goble-worker-{}", cn)
        } else {
            format!("goble-device-{}", cn)
        };
        params.distinguished_name = Self::dn("Goble", &name);
        params.extended_key_usages = if role.is_worker() {
            vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth]
        } else {
            vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth]
        };
        if add_worker_san {
            params.subject_alt_names = vec![
                SanType::DnsName(cn.try_into().unwrap()),
                SanType::IpAddress("127.0.0.1".parse().unwrap()),
            ];
        } else {
            params.subject_alt_names = vec![SanType::DnsName(cn.try_into().unwrap())];
        }
        params.serial_number = Some(SerialNumber::from(random_serial()));
        params.custom_extensions = vec![RoleExtension::make(role)];

        let key = KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let cert = params
            .signed_by(&key, &ca_cert, ca_key)
            .context("failed to sign certificate")?;
        let cert_pem = cert.pem();
        let key_pem = key.serialize_pem();
        let serial = extract_serial(&cert_pem)?;

        let identity = Identity {
            cert_pem: cert_pem.clone(),
            key_pem,
            serial,
            role,
        };
        self.store.write().unwrap().add(cert_pem)?;
        Ok(identity)
    }

    /// Revoke a certificate by serial number and bump the CRL version.
    pub fn revoke(&self, serial: &str) -> Result<()> {
        let mut store = self.store.write().unwrap();
        if store.is_active(serial) || !store.is_revoked(serial) {
            store.revoke(serial);
            store.crl_version += 1;
        }
        Ok(())
    }

    /// Build a signed CRL document from the current store.
    pub fn crl(&self) -> Result<SignedCrl> {
        self.store.read().unwrap().sign_crl(&self.identity.key_pem)
    }

    /// Apply a newer CRL to this CA's store.
    pub fn apply_crl(&self, crl: SignedCrl) -> Result<bool> {
        crl.verify(&self.identity.cert_pem)?;
        let mut store = self.store.write().unwrap();
        if crl.version <= store.crl_version {
            return Ok(false);
        }
        store.crl_version = crl.version;
        store.crl = Some(crl.clone());
        for serial in crl.revoked_serials {
            store.revoked.insert(serial);
        }
        let revoked = store.revoked.clone();
        store.active.retain(|serial, _| !revoked.contains(serial));
        Ok(true)
    }

    /// Verify that a certificate is signed by this CA, is not revoked, and has one of the
    /// allowed roles. Returns the role found.
    pub fn verify_role(&self, cert_pem: &str, allowed: &[ClusterRole]) -> Result<ClusterRole> {
        let der = pem_to_der(cert_pem)?;
        let (_, cert) = X509Certificate::from_der(&der)
            .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;

        let role = extract_role_from_cert(&cert)?;
        if !allowed.contains(&role) {
            anyhow::bail!("role {} is not allowed for this operation", role);
        }
        let serial = hex::encode(cert.serial.to_bytes_be());
        if self.store.read().unwrap().is_revoked(&serial) {
            anyhow::bail!("certificate {} is revoked", serial);
        }
        let (not_before, not_after) = (
            offset_to_chrono(cert.validity.not_before.to_datetime()),
            offset_to_chrono(cert.validity.not_after.to_datetime()),
        );
        let now = Utc::now();
        if now < not_before || now > not_after {
            anyhow::bail!("certificate {} is not valid at this time", serial);
        }
        // Verify the chain using the CA public key.
        let public_key_der = self.ca_public_key()?;
        let ca_public_key = UnparsedPublicKey::new(&ED25519, &public_key_der);
        let tbs = cert.tbs_certificate.as_ref();
        let sig = cert.signature_value.as_ref();
        if sig.is_empty() {
            anyhow::bail!("certificate signature is empty");
        }
        ca_public_key
            .verify(tbs, sig)
            .map_err(|_| anyhow::anyhow!("certificate signature is invalid"))?;
        Ok(role)
    }

    /// Verify that a certificate is signed by this CA, is not revoked, and has the Worker role.
    pub fn verify_worker(&self, cert_pem: &str) -> Result<()> {
        self.verify_role(cert_pem, &[ClusterRole::Worker])?;
        Ok(())
    }

    /// Verify that a client device certificate is valid and has an administrative role.
    pub fn verify_admin(&self, cert_pem: &str) -> Result<ClusterRole> {
        self.verify_role(cert_pem, &[ClusterRole::Owner, ClusterRole::Admin])
    }

    /// Verify that a client device certificate is valid for controlling workers.
    pub fn verify_controller(&self, cert_pem: &str) -> Result<ClusterRole> {
        self.verify_role(
            cert_pem,
            &[
                ClusterRole::Owner,
                ClusterRole::Admin,
                ClusterRole::Operator,
            ],
        )
    }

    /// Build a rustls server config that presents `server_identity` and requires a client
    /// certificate with one of the allowed roles.
    pub fn server_config(
        &self,
        server_identity: &Identity,
        allowed_roles: Vec<ClusterRole>,
    ) -> Result<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = self.client_verifier(allowed_roles, provider.clone())?;
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| anyhow::anyhow!("protocol versions: {e}"))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                server_identity.cert_chain()?,
                server_identity.private_key()?,
            )
            .context("failed to build server config")?;
        Ok(config)
    }

    /// Build a rustls client config that verifies the server has `expected_role` and presents
    /// `client_identity` for mutual authentication.
    pub fn client_config(
        &self,
        client_identity: &Identity,
        expected_server_role: ClusterRole,
    ) -> Result<rustls::ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let root_store = self.root_cert_store()?;
        let inner: Arc<dyn ServerCertVerifier> =
            rustls::client::WebPkiServerVerifier::builder_with_provider(
                root_store,
                provider.clone(),
            )
            .build()
            .context("failed to build webpki server verifier")?;
        let verifier = Arc::new(ClusterServerVerifier::new(inner, expected_server_role));
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| anyhow::anyhow!("protocol versions: {e}"))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_auth_cert(
                client_identity.cert_chain()?,
                client_identity.private_key()?,
            )
            .context("failed to build client config")?;
        Ok(config)
    }

    fn client_verifier(
        &self,
        allowed_roles: Vec<ClusterRole>,
        provider: Arc<rustls::crypto::CryptoProvider>,
    ) -> Result<Arc<ClusterClientVerifier>> {
        let root_store = self.root_cert_store()?;
        let inner =
            rustls::server::WebPkiClientVerifier::builder_with_provider(root_store, provider)
                .build()
                .context("failed to build webpki client verifier")?;
        Ok(Arc::new(ClusterClientVerifier::new(
            inner,
            allowed_roles,
            self.store.clone(),
        )))
    }

    pub fn root_cert_store(&self) -> Result<Arc<rustls::RootCertStore>> {
        self.identity.root_cert_store()
    }

    fn ca_public_key(&self) -> Result<Vec<u8>> {
        let der = pem_to_der(&self.identity.cert_pem)?;
        let (_, x509) = X509Certificate::from_der(&der)
            .map_err(|e| anyhow::anyhow!("failed to parse CA certificate: {e}"))?;
        let pk = x509.subject_pki.subject_public_key.as_ref();
        if pk.is_empty() {
            anyhow::bail!("invalid CA public key");
        }
        Ok(pk.to_vec())
    }

    fn base_params(days: u64) -> CertificateParams {
        let mut params = CertificateParams::default();
        let now = ::time::OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now + ::time::Duration::days(days as i64);
        params.use_authority_key_identifier_extension = true;
        params
    }

    fn dn(org: &str, cn: &str) -> rcgen::DistinguishedName {
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(DnType::OrganizationName, org);
        dn.push(DnType::CommonName, cn);
        dn
    }
}

pub(super) fn load_ed25519_key(key_pem: &str) -> Result<Ed25519KeyPair> {
    let mut bytes = key_pem.as_bytes();
    let key = rustls_pemfile::private_key(&mut bytes)?
        .ok_or_else(|| anyhow::anyhow!("no private key found in PEM"))?;
    let pkcs8 = key.secret_der().to_vec();
    Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| anyhow::anyhow!("invalid Ed25519 key"))
}

fn random_serial() -> Vec<u8> {
    let mut bytes = [0u8; 16];
    rand::fill(&mut bytes);
    // Clear the high bit to keep the serial positive.
    bytes[0] &= 0x7f;
    bytes.to_vec()
}
