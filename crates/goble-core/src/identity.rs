use anyhow::{Context, Result};
use base64::Engine;
use chrono::{DateTime, Utc};
use rcgen::{
    CertificateParams, CustomExtension, DnType, IsCa, KeyPair, SanType, SerialNumber,
};
use ring::signature::{Ed25519KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, RwLock};
use x509_parser::prelude::*;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DistinguishedName, DigitallySignedStruct};

/// String form of the Goble role extension OID.
const GOBLE_ROLE_OID_STR: &str = "1.3.6.1.4.1.42069.100.1.1";
/// Arc components of the Goble role extension OID.
pub const GOBLE_ROLE_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 42069, 100, 1, 1];

/// Hierarchical role embedded in every cluster certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ClusterRole {
    Owner,
    Admin,
    Operator,
    Viewer,
    Worker,
}

impl ClusterRole {
    pub fn is_device(self) -> bool {
        !matches!(self, Self::Worker)
    }

    pub fn is_worker(self) -> bool {
        matches!(self, Self::Worker)
    }

    pub fn can_manage_cluster(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    pub fn can_operate(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Operator)
    }
}

impl fmt::Display for ClusterRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::Operator => "Operator",
            Self::Viewer => "Viewer",
            Self::Worker => "Worker",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for ClusterRole {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Owner" => Ok(Self::Owner),
            "Admin" => Ok(Self::Admin),
            "Operator" => Ok(Self::Operator),
            "Viewer" => Ok(Self::Viewer),
            "Worker" => Ok(Self::Worker),
            _ => anyhow::bail!("unknown cluster role: {}", s),
        }
    }
}

/// Non-critical X.509 custom extension that carries the certificate role.
struct RoleExtension;

impl RoleExtension {
    fn make(role: ClusterRole) -> CustomExtension {
        let content = yasna::construct_der(|writer| {
            writer.write_utf8string(role.to_string().as_str());
        });
        CustomExtension::from_oid_content(GOBLE_ROLE_OID, content)
    }
}

/// Extract the Goble role from a PEM certificate.
pub fn extract_role(cert_pem: &str) -> Result<ClusterRole> {
    let der = pem_to_der(cert_pem)?;
    let (_, x509) = X509Certificate::from_der(&der)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;
    extract_role_from_cert(&x509)
}

fn extract_role_from_cert(x509: &X509Certificate<'_>) -> Result<ClusterRole> {
    for ext in x509.extensions() {
        if ext.oid.to_id_string() == GOBLE_ROLE_OID_STR {
            return parse_utf8_der(ext.value)
                .and_then(|s| s.parse::<ClusterRole>())
                .context("invalid role extension content");
        }
    }
    anyhow::bail!("certificate does not contain a Goble role extension")
}

/// Extract the serial number as a lowercase hex string.
pub fn extract_serial(cert_pem: &str) -> Result<String> {
    let der = pem_to_der(cert_pem)?;
    let (_, x509) = X509Certificate::from_der(&der)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;
    Ok(hex::encode(x509.serial.to_bytes_be()))
}

/// Extract certificate validity window.
pub fn extract_validity(cert_pem: &str) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let der = pem_to_der(cert_pem)?;
    let (_, x509) = X509Certificate::from_der(&der)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;
    let not_before = x509.validity.not_before.to_datetime();
    let not_after = x509.validity.not_after.to_datetime();
    Ok((
        offset_to_chrono(not_before),
        offset_to_chrono(not_after),
    ))
}

fn offset_to_chrono(dt: ::time::OffsetDateTime) -> DateTime<Utc> {
    DateTime::from_timestamp(dt.unix_timestamp(), dt.nanosecond())
        .expect("valid timestamp")
}

fn parse_utf8_der(bytes: &[u8]) -> Result<String> {
    if bytes.len() < 2 || bytes[0] != 0x0c {
        anyhow::bail!("expected DER UTF8String");
    }
    let (len, consumed) = parse_ber_length(&bytes[1..])?;
    let start = 1 + consumed;
    if start + len > bytes.len() {
        anyhow::bail!("DER UTF8String length out of bounds");
    }
    String::from_utf8(bytes[start..start + len].to_vec())
        .map_err(|e| anyhow::anyhow!("invalid UTF8String: {e}"))
}

fn parse_ber_length(bytes: &[u8]) -> Result<(usize, usize)> {
    if bytes.is_empty() {
        anyhow::bail!("missing length octet");
    }
    let first = bytes[0];
    if first & 0x80 == 0 {
        return Ok((first as usize, 1));
    }
    let num_bytes = (first & 0x7f) as usize;
    if num_bytes == 0 || num_bytes > 8 || bytes.len() < 1 + num_bytes {
        anyhow::bail!("invalid long-form length");
    }
    let mut len = 0usize;
    for b in &bytes[1..1 + num_bytes] {
        len = len
            .checked_shl(8)
            .and_then(|l| l.checked_add(*b as usize))
            .context("length overflow")?;
    }
    Ok((len, 1 + num_bytes))
}

/// Parse a PEM string into DER bytes.
pub fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let (_, parsed) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to parse PEM: {e}"))?;
    Ok(parsed.contents)
}

/// An issued certificate plus its private key in PEM form.
#[derive(Debug, Clone)]
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
            store.add(cert).context("failed to add cert to root store")?;
        }
        Ok(Arc::new(store))
    }
}

/// A signed CRL-like document that lists revoked certificate serials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCrl {
    pub version: u64,
    pub issued_at: DateTime<Utc>,
    pub revoked_serials: Vec<String>,
    pub signature: String,
}

impl SignedCrl {
    fn canonical_bytes(&self) -> Vec<u8> {
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

/// Custom rustls client certificate verifier that delegates chain validation to webpki and then
/// enforces the Goble role extension and revocation state.
#[derive(Debug, Clone)]
pub struct ClusterClientVerifier {
    inner: Arc<dyn ClientCertVerifier>,
    allowed_roles: Vec<ClusterRole>,
    store: Arc<RwLock<CertificateStore>>,
}

impl ClusterClientVerifier {
    pub fn new(
        inner: Arc<dyn ClientCertVerifier>,
        allowed_roles: Vec<ClusterRole>,
        store: Arc<RwLock<CertificateStore>>,
    ) -> Self {
        Self {
            inner,
            allowed_roles,
            store,
        }
    }
}

impl ClientCertVerifier for ClusterClientVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        now: rustls::pki_types::UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.inner
            .verify_client_cert(end_entity, intermediates, now)?;

        let (_, x509) = X509Certificate::from_der(end_entity.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        let role = extract_role_from_cert(&x509).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        if !self.allowed_roles.contains(&role) {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        let serial = hex::encode(x509.serial.to_bytes_be());
        if self.store.read().unwrap().is_revoked(&serial) {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::Revoked,
            ));
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Custom rustls server certificate verifier that delegates chain validation to webpki and then
/// enforces the expected Goble role extension on the server certificate.
#[derive(Debug, Clone)]
pub struct ClusterServerVerifier {
    inner: Arc<dyn ServerCertVerifier>,
    expected_role: ClusterRole,
}

impl ClusterServerVerifier {
    pub fn new(
        inner: Arc<dyn ServerCertVerifier>,
        expected_role: ClusterRole,
    ) -> Self {
        Self {
            inner,
            expected_role,
        }
    }
}

impl ServerCertVerifier for ClusterServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        let (_, x509) = X509Certificate::from_der(end_entity.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        let role = extract_role_from_cert(&x509).map_err(|_| {
            rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)
        })?;
        if role != self.expected_role {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

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
    pub fn from_key(cluster_key: &crate::cluster_key::ClusterKey, cluster_name: impl AsRef<str>) -> Result<Self> {
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
        let ca_params = CertificateParams::from_ca_cert_der(
            &pem_to_der(&identity.cert_pem)?.into(),
        )
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
            .with_single_cert(server_identity.cert_chain()?, server_identity.private_key()?)
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
            .with_client_auth_cert(client_identity.cert_chain()?, client_identity.private_key()?)
            .context("failed to build client config")?;
        Ok(config)
    }

    fn client_verifier(
        &self,
        allowed_roles: Vec<ClusterRole>,
        provider: Arc<rustls::crypto::CryptoProvider>,
    ) -> Result<Arc<ClusterClientVerifier>> {
        let root_store = self.root_cert_store()?;
        let inner = rustls::server::WebPkiClientVerifier::builder_with_provider(
            root_store,
            provider,
        )
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

fn load_ed25519_key(key_pem: &str) -> Result<Ed25519KeyPair> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    fn make_ca() -> ClusterCa {
        ClusterCa::generate_new("test-cluster").unwrap()
    }

    #[test]
    fn test_ca_generation() {
        let ca = make_ca();
        assert!(ca.identity.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(ca.identity.key_pem.contains("BEGIN PRIVATE KEY"));
        assert_eq!(ca.identity.role, ClusterRole::Owner);
    }

    #[test]
    fn test_sign_device() {
        let ca = make_ca();
        let device = ca
            .sign_device("desktop-1", ClusterRole::Admin, 365)
            .unwrap();
        assert!(device.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(device.key_pem.contains("BEGIN PRIVATE KEY"));
        assert_eq!(device.role, ClusterRole::Admin);
        assert_eq!(extract_role(&device.cert_pem).unwrap(), ClusterRole::Admin);
    }

    #[test]
    fn test_sign_worker() {
        let ca = make_ca();
        let worker = ca.sign_worker("worker-1", 365).unwrap();
        assert_eq!(worker.role, ClusterRole::Worker);
        ca.verify_worker(&worker.cert_pem).unwrap();
    }

    #[test]
    fn test_verify_role_rejects_wrong_role() {
        let ca = make_ca();
        let viewer = ca
            .sign_device("viewer-1", ClusterRole::Viewer, 365)
            .unwrap();
        assert!(ca.verify_admin(&viewer.cert_pem).is_err());
        assert!(ca.verify_controller(&viewer.cert_pem).is_err());
    }

    #[test]
    fn test_revoke_and_crl() {
        let ca = make_ca();
        let device = ca
            .sign_device("device-1", ClusterRole::Operator, 365)
            .unwrap();
        let serial = device.serial().to_string();
        ca.revoke(&serial).unwrap();
        let crl = ca.crl().unwrap();
        assert_eq!(crl.version, 1);
        assert!(crl.revoked_serials.contains(&serial));
        crl.verify(&ca.identity.cert_pem).unwrap();
        assert!(ca.verify_controller(&device.cert_pem).is_err());
    }

    #[test]
    fn test_crl_apply_updates_store() {
        let ca = make_ca();
        let device = ca
            .sign_device("device-1", ClusterRole::Admin, 365)
            .unwrap();
        let serial = device.serial().to_string();
        ca.revoke(&serial).unwrap();
        let crl = ca.crl().unwrap();

        let other_ca = ClusterCa::from_ca_cert_pem(&ca.identity.cert_pem).unwrap();
        other_ca.store.write().unwrap().add_identity(&ca.identity).unwrap();
        other_ca.store.write().unwrap().add_identity(&device).unwrap();
        assert!(other_ca.apply_crl(crl).unwrap());
        assert!(other_ca.store.read().unwrap().is_revoked(&serial));
        assert!(!other_ca.store.read().unwrap().is_active(&serial));
    }

    #[test]
    fn test_certificate_store_rejects_revoked() {
        let ca = make_ca();
        let device = ca
            .sign_device("device-1", ClusterRole::Admin, 365)
            .unwrap();
        let serial = device.serial().to_string();
        ca.revoke(&serial).unwrap();
        assert!(ca.store.write().unwrap().add(device.cert_pem.clone()).is_err());
    }

    #[test]
    fn test_role_ordering_hierarchy() {
        assert!(ClusterRole::Owner.can_manage_cluster());
        assert!(ClusterRole::Admin.can_manage_cluster());
        assert!(!ClusterRole::Operator.can_manage_cluster());
        assert!(ClusterRole::Operator.can_operate());
        assert!(!ClusterRole::Viewer.can_operate());
        assert!(!ClusterRole::Worker.can_operate());
    }

    #[test]
    fn test_tls_configs_build() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca = make_ca();
        let worker = ca.sign_worker("worker-1", 365).unwrap();
        let desktop = ca.sign_device("desktop-1", ClusterRole::Admin, 365).unwrap();

        let server_config = ca
            .server_config(&worker, vec![ClusterRole::Admin, ClusterRole::Owner])
            .unwrap();
        let client_config = ca.client_config(&desktop, ClusterRole::Worker).unwrap();
        assert!(server_config.alpn_protocols.is_empty());
        assert!(client_config.alpn_protocols.is_empty());
    }

    struct Pipe {
        reader: Arc<Mutex<VecDeque<u8>>>,
        writer: Arc<Mutex<VecDeque<u8>>>,
    }

    fn pipe_pair() -> (Pipe, Pipe) {
        let a = Arc::new(Mutex::new(VecDeque::new()));
        let b = Arc::new(Mutex::new(VecDeque::new()));
        (
            Pipe {
                reader: a.clone(),
                writer: b.clone(),
            },
            Pipe {
                reader: b,
                writer: a,
            },
        )
    }

    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut reader = self.reader.lock().unwrap();
            let mut n = 0;
            while n < buf.len() && !reader.is_empty() {
                buf[n] = reader.pop_front().unwrap();
                n += 1;
            }
            Ok(n)
        }
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut writer = self.writer.lock().unwrap();
            writer.extend(buf.iter());
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn run_handshake(
        client: &mut rustls::ClientConnection,
        server: &mut rustls::ServerConnection,
        mut client_pipe: Pipe,
        mut server_pipe: Pipe,
    ) -> Result<(), rustls::Error> {
        for _ in 0..100 {
            if !client.is_handshaking() && !server.is_handshaking() {
                return Ok(());
            }
            let _ = client.write_tls(&mut client_pipe);
            let _ = server.read_tls(&mut server_pipe);
            server.process_new_packets()?;
            let _ = server.write_tls(&mut server_pipe);
            let _ = client.read_tls(&mut client_pipe);
            client.process_new_packets()?;
        }
        Err(rustls::Error::General("handshake did not complete".into()))
    }

    #[test]
    fn test_mtls_handshake_accepts_controller_client() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca = make_ca();
        let worker = ca.sign_worker("worker-1", 365).unwrap();
        let desktop = ca.sign_device("desktop-1", ClusterRole::Admin, 365).unwrap();
        let server_config = Arc::new(
            ca.server_config(
                &worker,
                vec![
                    ClusterRole::Owner,
                    ClusterRole::Admin,
                    ClusterRole::Operator,
                ],
            )
            .unwrap(),
        );
        let client_config = Arc::new(ca.client_config(&desktop, ClusterRole::Worker).unwrap());
        let server_name = "worker-1".try_into().unwrap();
        let mut server = rustls::ServerConnection::new(server_config).unwrap();
        let mut client = rustls::ClientConnection::new(client_config, server_name).unwrap();
        let (client_pipe, server_pipe) = pipe_pair();
        run_handshake(&mut client, &mut server, client_pipe, server_pipe).unwrap();
    }

    #[test]
    fn test_mtls_handshake_rejects_worker_client_on_server() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca = make_ca();
        let worker = ca.sign_worker("worker-1", 365).unwrap();
        let malicious_client = ca.sign_worker("bad-1", 365).unwrap();
        let server_config = Arc::new(
            ca.server_config(
                &worker,
                vec![
                    ClusterRole::Owner,
                    ClusterRole::Admin,
                    ClusterRole::Operator,
                ],
            )
            .unwrap(),
        );
        let client_config =
            Arc::new(ca.client_config(&malicious_client, ClusterRole::Worker).unwrap());
        let server_name = "worker-1".try_into().unwrap();
        let mut server = rustls::ServerConnection::new(server_config).unwrap();
        let mut client = rustls::ClientConnection::new(client_config, server_name).unwrap();
        let (client_pipe, server_pipe) = pipe_pair();
        assert!(run_handshake(&mut client, &mut server, client_pipe, server_pipe).is_err());
    }
}

