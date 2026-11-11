//! Pairing a new device without a shared PIN.
//!
//! The trusted device creates a one-time offer: a 128-bit secret, a fresh
//! X25519 key, an expiry and its own device id. The QR code carries all of it.
//! The new device answers with its own X25519 key, its Ed25519 device key and a
//! proof that it holds both. Both ends derive the same key, compare a six-digit
//! SAS and then use the derived frame keys to carry the issued credential.
//!
//! The relay sees the offer id, the public keys and frame sizes. It never sees
//! the secret, the shared key or the credential.

use std::collections::HashMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ring::hkdf::{Salt, HKDF_SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::{CryptoError, EphemeralSecret, Session};

/// The only pairing payload version this crate accepts.
pub const PAIRING_VERSION: u8 = 1;
/// How long an offer stays usable.
pub const OFFER_TTL_SECS: i64 = 120;
/// The scheme and host of a pairing URI.
const PAIRING_URI_PREFIX: &str = "goble://pair";

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("the QR payload is not a Goble pairing URI")]
    NotPairingUri,
    #[error("unsupported pairing version {0}")]
    UnsupportedVersion(u8),
    #[error("the pairing offer expired")]
    Expired,
    #[error("the pairing offer was already used")]
    Used,
    #[error("the pairing request names a different offer")]
    WrongOffer,
    #[error("the pairing request was not authenticated")]
    BadConfirmation,
    #[error("the joining device did not prove its key")]
    BadDeviceProof,
    #[error("the pairing offer is unknown")]
    UnknownOffer,
    #[error("the pairing offer was already completed")]
    Closed,
    #[error("the pairing proof does not match")]
    MismatchedProof,
    #[error("malformed pairing field: {0}")]
    Malformed(&'static str),
    #[error("the credential envelope is malformed: {0}")]
    Credential(String),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

/// The payload a trusted device shows as a QR code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingInvite {
    pub version: u8,
    pub relay: String,
    pub offer_id: String,
    pub initiator_device: String,
    pub initiator_key: String,
    pub secret: String,
    pub expires_at: i64,
}

impl PairingInvite {
    pub fn offer_id(&self) -> &str {
        &self.offer_id
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    pub fn relay(&self) -> &str {
        &self.relay
    }

    /// The raw 128-bit secret. The value is only ever shown inside the QR code
    /// and inside the pairing ceremony; the relay is given its hash.
    pub fn secret_bytes(&self) -> Result<[u8; 16], PairingError> {
        decode_fixed::<16>(&self.secret).ok_or(PairingError::Malformed("secret"))
    }

    pub fn initiator_key_bytes(&self) -> Result<[u8; 32], PairingError> {
        decode_fixed::<32>(&self.initiator_key).ok_or(PairingError::Malformed("initiator key"))
    }

    /// `SHA-256(secret)`. This is what the relay stores, so the QR secret is
    /// not sufficient to claim the offer at the relay.
    pub fn secret_hash(&self) -> Result<[u8; 32], PairingError> {
        let secret = self.secret_bytes()?;
        Ok(sha256(&secret))
    }

    pub fn ensure_fresh(&self, now: DateTime<Utc>) -> Result<(), PairingError> {
        if now.timestamp() > self.expires_at {
            return Err(PairingError::Expired);
        }
        Ok(())
    }

    /// Encode the QR payload. Values are percent-encoded because a relay URL
    /// contains `:` and `/`.
    pub fn to_uri(&self) -> String {
        format!(
            "{PAIRING_URI_PREFIX}?v={}&r={}&o={}&d={}&k={}&s={}&e={}",
            self.version,
            encode_component(&self.relay),
            encode_component(&self.offer_id),
            encode_component(&self.initiator_device),
            encode_component(&self.initiator_key),
            encode_component(&self.secret),
            self.expires_at,
        )
    }

    /// Parse a scanned QR payload.
    pub fn parse(uri: &str) -> Result<Self, PairingError> {
        let query = uri
            .strip_prefix(PAIRING_URI_PREFIX)
            .and_then(|rest| rest.strip_prefix('?'))
            .ok_or(PairingError::NotPairingUri)?;
        let mut fields = HashMap::new();
        for pair in query.split('&') {
            let (key, value) = pair
                .split_once('=')
                .ok_or(PairingError::Malformed("query field"))?;
            let value = decode_component(value)?;
            if fields.insert(key.to_string(), value).is_some() {
                return Err(PairingError::Malformed("duplicate query field"));
            }
        }
        let mut take =
            |name: &'static str| fields.remove(name).ok_or(PairingError::Malformed(name));
        let version: u8 = take("v")?
            .parse()
            .map_err(|_| PairingError::Malformed("version"))?;
        if version != PAIRING_VERSION {
            return Err(PairingError::UnsupportedVersion(version));
        }
        let expires_at: i64 = take("e")?
            .parse()
            .map_err(|_| PairingError::Malformed("expiry"))?;
        let invite = Self {
            version,
            relay: take("r")?,
            offer_id: take("o")?,
            initiator_device: take("d")?,
            initiator_key: take("k")?,
            secret: take("s")?,
            expires_at,
        };
        if invite.offer_id.is_empty() || invite.initiator_device.is_empty() {
            return Err(PairingError::Malformed("identifier"));
        }
        // Validate the key material now, so a malformed QR fails before the
        // user compares a SAS that could never match.
        invite.secret_bytes()?;
        invite.initiator_key_bytes()?;
        Ok(invite)
    }
}

/// The trusted device's side of one offer.
pub struct PairingOffer {
    invite: PairingInvite,
    secret: [u8; 16],
    ephemeral: Option<EphemeralSecret>,
}

impl PairingOffer {
    /// Create an offer. `ttl` is clamped to the two-minute window the design
    /// uses; a longer-lived QR widens the shoulder-surfing window.
    pub fn create(
        relay: impl Into<String>,
        initiator_device: impl Into<String>,
        now: DateTime<Utc>,
        ttl: std::time::Duration,
    ) -> Result<Self, PairingError> {
        let ttl = ttl.as_secs().min(OFFER_TTL_SECS as u64).max(1) as i64;
        let secret = random::<16>();
        let offer_id = URL_SAFE_NO_PAD.encode(random::<16>());
        let ephemeral = EphemeralSecret::generate()?;
        let initiator_key = URL_SAFE_NO_PAD.encode(ephemeral.public_key());
        Ok(Self {
            invite: PairingInvite {
                version: PAIRING_VERSION,
                relay: relay.into(),
                offer_id,
                initiator_device: initiator_device.into(),
                initiator_key,
                secret: URL_SAFE_NO_PAD.encode(secret),
                expires_at: now.timestamp() + ttl,
            },
            secret,
            ephemeral: Some(ephemeral),
        })
    }

    pub fn invite(&self) -> &PairingInvite {
        &self.invite
    }

    pub fn uri(&self) -> String {
        self.invite.to_uri()
    }

    /// The value the relay records for this offer.
    pub fn secret_hash(&self) -> [u8; 32] {
        sha256(&self.secret)
    }

    /// Verify the joining device's proof and derive the shared channel.
    ///
    /// The ephemeral key is consumed, so a second request for the same offer
    /// cannot derive a second session.
    pub fn accept(
        &mut self,
        request: &PairingRequest,
        now: DateTime<Utc>,
    ) -> Result<AcceptedPairing, PairingError> {
        self.invite.ensure_fresh(now)?;
        if request.offer_id != self.invite.offer_id {
            return Err(PairingError::WrongOffer);
        }
        let ephemeral = self.ephemeral.take().ok_or(PairingError::Closed)?;
        let joiner_key = decode_fixed::<32>(&request.ephemeral_key)
            .ok_or(PairingError::Malformed("joining key"))?;
        let joiner_device_key =
            decode_fixed::<32>(&request.device_key).ok_or(PairingError::Malformed("device key"))?;
        let transcript = transcript(
            &self.invite.offer_id,
            &self.invite.initiator_device,
            self.invite.initiator_key_bytes()?,
            &request.device_id,
            joiner_key,
            joiner_device_key,
        );
        verify_ed25519(&joiner_device_key, &transcript, &request.device_signature)
            .map_err(|_| PairingError::BadDeviceProof)?;

        let session = ephemeral.finish(&joiner_key, &transcript)?;
        let key = pairing_key(&session.shared, &transcript, &self.secret)?;
        let presented = decode_fixed::<32>(&request.confirmation)
            .ok_or(PairingError::Malformed("confirmation"))?;
        let expected = mac(&key, b"joiner", &transcript);
        if !constant_time_eq(&presented, &expected) {
            return Err(PairingError::BadConfirmation);
        }

        let sas = sas(&key, &transcript);
        let acceptance = PairingAcceptance {
            offer_id: self.invite.offer_id.clone(),
            confirmation: URL_SAFE_NO_PAD.encode(mac(&key, b"initiator", &transcript)),
            sas: sas.clone(),
        };
        Ok(AcceptedPairing {
            device_id: request.device_id.clone(),
            device_key: joiner_device_key,
            sas,
            session,
            acceptance,
        })
    }
}

/// What the trusted device gets after a successful join. It confirms the SAS
/// with the user, then seals the issued credential with `session.s2c`.
pub struct AcceptedPairing {
    pub device_id: String,
    pub device_key: [u8; 32],
    pub sas: String,
    pub session: Session,
    pub acceptance: PairingAcceptance,
}

/// The material the trusted device sends over the pairing channel after the
/// user confirms the SAS. It is encrypted with the pairing session key, so the
/// relay never sees the certificate or its private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCredential {
    pub device_id: String,
    pub serial: String,
    pub role: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_cert_pem: String,
}

impl AcceptedPairing {
    /// Seal the issued credential for the joining device.
    pub fn seal_credential(
        &mut self,
        credential: &DeviceCredential,
    ) -> Result<Vec<u8>, PairingError> {
        let encoded = serde_json::to_vec(credential)
            .map_err(|error| PairingError::Credential(error.to_string()))?;
        Ok(self.session.s2c.seal(&encoded)?)
    }
}

/// The response the trusted device sends back over the rendezvous channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingAcceptance {
    pub offer_id: String,
    pub confirmation: String,
    pub sas: String,
}

/// The joining device's request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub offer_id: String,
    pub device_id: String,
    pub device_key: String,
    pub ephemeral_key: String,
    pub device_signature: String,
    pub confirmation: String,
}

/// The new device's side of the ceremony.
pub struct PairingJoiner {
    invite: PairingInvite,
    request: PairingRequest,
    session: Option<Session>,
    key: [u8; 32],
    transcript: [u8; 32],
    device_key_pkcs8: Vec<u8>,
}

impl PairingJoiner {
    /// Build the request a new device sends to the trusted device.
    pub fn begin(
        invite: &PairingInvite,
        device_id: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Self, PairingError> {
        invite.ensure_fresh(now)?;
        let device_id = device_id.into();
        if device_id.is_empty() {
            return Err(PairingError::Malformed("device id"));
        }
        let initiator_key = invite.initiator_key_bytes()?;
        let secret = invite.secret_bytes()?;
        let ephemeral = EphemeralSecret::generate()?;
        let ephemeral_key = ephemeral.public_key();

        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| PairingError::Crypto(CryptoError::Handshake("ed25519 key".into())))?;
        let device_key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
            .map_err(|_| PairingError::Crypto(CryptoError::Handshake("ed25519 key".into())))?;
        let device_public: [u8; 32] = device_key
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| PairingError::Malformed("device key"))?;

        let transcript = transcript(
            &invite.offer_id,
            &invite.initiator_device,
            initiator_key,
            &device_id,
            ephemeral_key,
            device_public,
        );
        let signature = device_key.sign(&transcript);
        let session = ephemeral.finish(&initiator_key, &transcript)?;
        let key = pairing_key(&session.shared, &transcript, &secret)?;
        let request = PairingRequest {
            offer_id: invite.offer_id.clone(),
            device_id,
            device_key: URL_SAFE_NO_PAD.encode(device_public),
            ephemeral_key: URL_SAFE_NO_PAD.encode(ephemeral_key),
            device_signature: URL_SAFE_NO_PAD.encode(signature.as_ref()),
            confirmation: URL_SAFE_NO_PAD.encode(mac(&key, b"joiner", &transcript)),
        };
        Ok(Self {
            invite: invite.clone(),
            request,
            session: Some(session),
            key,
            transcript,
            device_key_pkcs8: pkcs8.as_ref().to_vec(),
        })
    }

    pub fn request(&self) -> &PairingRequest {
        &self.request
    }

    /// The Ed25519 private key in PKCS#8 form. The caller stores it in the
    /// device keystore; the public half is in the request.
    pub fn device_key_pkcs8(&self) -> &[u8] {
        &self.device_key_pkcs8
    }

    /// Verify the trusted device's confirmation and SAS, yielding the channel
    /// that carries the issued credential.
    pub fn complete(
        self,
        acceptance: &PairingAcceptance,
    ) -> Result<EstablishedPairing, PairingError> {
        if acceptance.offer_id != self.invite.offer_id {
            return Err(PairingError::WrongOffer);
        }
        let session = self.session.ok_or(PairingError::Closed)?;
        let expected_sas = sas(&self.key, &self.transcript);
        if acceptance.sas != expected_sas {
            return Err(PairingError::MismatchedProof);
        }
        let presented = decode_fixed::<32>(&acceptance.confirmation)
            .ok_or(PairingError::Malformed("confirmation"))?;
        let expected = mac(&self.key, b"initiator", &self.transcript);
        if !constant_time_eq(&presented, &expected) {
            return Err(PairingError::BadConfirmation);
        }
        let peer_device_key = self.invite.initiator_key_bytes()?;
        Ok(EstablishedPairing {
            device_id: self.request.device_id,
            peer_device_id: self.invite.initiator_device,
            peer_device_key,
            sas: expected_sas,
            session,
            device_key_pkcs8: self.device_key_pkcs8,
        })
    }
}

/// A completed pairing on the new device. The credential arrives sealed with
/// `session.c2s`/`session.s2c`; this object only exposes the channel.
pub struct EstablishedPairing {
    pub device_id: String,
    pub peer_device_id: String,
    pub peer_device_key: [u8; 32],
    pub sas: String,
    pub session: Session,
    pub device_key_pkcs8: Vec<u8>,
}

impl EstablishedPairing {
    /// Open the credential the trusted device sealed after the SAS check.
    pub fn open_credential(&mut self, frame: &[u8]) -> Result<DeviceCredential, PairingError> {
        let plaintext = self.session.s2c.open(frame)?;
        let credential: DeviceCredential = serde_json::from_slice(&plaintext)
            .map_err(|error| PairingError::Credential(error.to_string()))?;
        if credential.device_id != self.device_id {
            return Err(PairingError::Credential(
                "the credential names another device".into(),
            ));
        }
        if credential.serial.is_empty() || credential.cert_pem.is_empty() {
            return Err(PairingError::Credential(
                "the credential is missing its certificate".into(),
            ));
        }
        Ok(credential)
    }
}

/// One relay's view of outstanding offers. It stores only the secret hash.
#[derive(Default)]
pub struct OfferBook {
    offers: HashMap<String, PendingOffer>,
}

struct PendingOffer {
    secret_hash: [u8; 32],
    expires_at: i64,
    used: bool,
}

impl OfferBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an offer the trusted device announced. Re-registering an id is
    /// refused so a stale id cannot be revived with a new secret.
    pub fn reserve(&mut self, invite: &PairingInvite) -> Result<(), PairingError> {
        self.reserve_hash(
            invite.offer_id(),
            invite.secret_hash()?,
            invite.expires_at(),
        )
    }

    pub fn reserve_hash(
        &mut self,
        offer_id: impl Into<String>,
        secret_hash: [u8; 32],
        expires_at: i64,
    ) -> Result<(), PairingError> {
        let offer_id = offer_id.into();
        if self.offers.contains_key(&offer_id) {
            return Err(PairingError::Used);
        }
        self.offers.insert(
            offer_id,
            PendingOffer {
                secret_hash,
                expires_at,
                used: false,
            },
        );
        Ok(())
    }

    /// Claim an offer once. The relay checks the presented secret hash, the
    /// expiry and the single-use flag without ever learning the secret.
    pub fn claim(
        &mut self,
        offer_id: &str,
        secret_hash: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<(), PairingError> {
        let offer = self
            .offers
            .get_mut(offer_id)
            .ok_or(PairingError::UnknownOffer)?;
        if offer.used {
            return Err(PairingError::Used);
        }
        if now.timestamp() > offer.expires_at {
            return Err(PairingError::Expired);
        }
        if !constant_time_eq(&offer.secret_hash, secret_hash) {
            return Err(PairingError::MismatchedProof);
        }
        offer.used = true;
        Ok(())
    }

    /// Drop expired and used offers. Returning the number removed makes it
    /// visible in a relay's metrics.
    pub fn purge(&mut self, now: DateTime<Utc>) -> usize {
        let before = self.offers.len();
        self.offers
            .retain(|_, offer| !offer.used && now.timestamp() <= offer.expires_at);
        before - self.offers.len()
    }

    pub fn len(&self) -> usize {
        self.offers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }
}

/// The six-digit authentication string, grouped for reading aloud.
fn sas(key: &[u8; 32], transcript: &[u8; 32]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"goble-pair-sas-v1");
    hash.update(key);
    hash.update(transcript);
    let digest = hash.finalize();
    let number = u32::from_be_bytes(digest[..4].try_into().expect("four bytes")) % 1_000_000;
    format!("{:03} {:03}", number / 1000, number % 1000)
}

fn pairing_key(
    shared: &[u8; 32],
    transcript: &[u8; 32],
    secret: &[u8; 16],
) -> Result<[u8; 32], PairingError> {
    let salt = Salt::new(HKDF_SHA256, transcript);
    let prk = salt.extract(shared);
    let mut key = [0u8; 32];
    // The QR secret is mixed into the key, so a device that only learned the
    // offer id from a relay cannot derive the same confirmation.
    prk.expand(&[b"goble-pair-v1", secret], HKDF_SHA256)
        .map_err(|_| PairingError::Malformed("pairing key"))?
        .fill(&mut key)
        .map_err(|_| PairingError::Malformed("pairing key"))?;
    Ok(key)
}

fn mac(key: &[u8; 32], label: &[u8], transcript: &[u8; 32]) -> [u8; 32] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut context = hmac::Context::with_key(&key);
    context.update(label);
    context.update(transcript);
    let tag = context.sign();
    tag.as_ref().try_into().expect("HMAC-SHA256 is 32 bytes")
}

/// `SHA-256("goble-pair-v1" || offer_id || initiator_device ||
/// initiator_key || joiner_device || joiner_key || joiner_device_key)`.
/// Every field is length-prefixed so two concatenations cannot collide.
fn transcript(
    offer_id: &str,
    initiator_device: &str,
    initiator_key: [u8; 32],
    joiner_device: &str,
    joiner_key: [u8; 32],
    joiner_device_key: [u8; 32],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"goble-pair-v1");
    for field in [offer_id.as_bytes(), initiator_device.as_bytes()] {
        hash.update((field.len() as u32).to_be_bytes());
        hash.update(field);
    }
    hash.update(initiator_key);
    let joiner = joiner_device.as_bytes();
    hash.update((joiner.len() as u32).to_be_bytes());
    hash.update(joiner);
    hash.update(joiner_key);
    hash.update(joiner_device_key);
    hash.finalize().into()
}

fn verify_ed25519(public_key: &[u8; 32], message: &[u8], signature: &str) -> Result<(), ()> {
    let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| ())?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(message, &signature)
        .map_err(|_| ())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

fn decode_fixed<const N: usize>(value: &str) -> Option<[u8; N]> {
    let bytes = URL_SAFE_NO_PAD.decode(value.trim()).ok()?;
    bytes.try_into().ok()
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("the system random source is available");
    bytes
}

fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn decode_component(value: &str) -> Result<String, PairingError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(PairingError::Malformed("percent escape"));
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|_| PairingError::Malformed("percent escape"))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| PairingError::Malformed("percent escape"))?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| PairingError::Malformed("percent escape"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn offer() -> PairingOffer {
        PairingOffer::create(
            "https://relay.goble.dev/v1",
            "desktop",
            now(),
            std::time::Duration::from_secs(120),
        )
        .expect("an offer")
    }

    fn ceremony() -> (PairingOffer, AcceptedPairing, EstablishedPairing) {
        let mut trusted = offer();
        let invite = trusted.invite().clone();
        let joiner = PairingJoiner::begin(&invite, "phone", now()).expect("a request");
        let request = joiner.request().clone();
        let accepted = trusted.accept(&request, now()).expect("the join");
        let established = joiner
            .complete(&accepted.acceptance)
            .expect("the confirmation");
        (trusted, accepted, established)
    }

    #[test]
    fn the_qr_uri_round_trips() {
        let trusted = offer();
        let uri = trusted.uri();
        let parsed = PairingInvite::parse(&uri).expect("a parsed invite");
        assert_eq!(&parsed, trusted.invite());
        assert_eq!(parsed.secret_hash().unwrap(), trusted.secret_hash());
        assert_eq!(parsed.relay(), "https://relay.goble.dev/v1");
    }

    #[test]
    fn a_qr_from_another_scheme_is_refused() {
        assert!(matches!(
            PairingInvite::parse("https://example.com/?v=1"),
            Err(PairingError::NotPairingUri)
        ));
    }

    #[test]
    fn an_unsupported_version_is_refused() {
        let uri = offer().uri().replace("v=1", "v=2");
        assert!(matches!(
            PairingInvite::parse(&uri),
            Err(PairingError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn a_wrong_qr_secret_cannot_produce_a_valid_confirmation() {
        let mut trusted = offer();
        let mut invite = trusted.invite().clone();
        invite.secret = URL_SAFE_NO_PAD.encode([9u8; 16]);
        assert!(PairingInvite::parse(&invite.to_uri()).is_ok());
        let joiner = PairingJoiner::begin(&invite, "phone", now()).unwrap();
        assert!(matches!(
            trusted.accept(joiner.request(), now()),
            Err(PairingError::BadConfirmation)
        ));
    }

    #[test]
    fn both_ends_show_the_same_sas() {
        let (_trusted, accepted, established) = ceremony();
        assert_eq!(accepted.sas, established.sas);
        assert_eq!(accepted.sas.len(), 7);
        assert_eq!(accepted.sas.as_bytes()[3], b' ');
    }

    #[test]
    fn the_joining_device_can_open_the_credential_channel() {
        let (_trusted, mut accepted, mut established) = ceremony();
        let credential = DeviceCredential {
            device_id: "phone".into(),
            serial: "serial-1".into(),
            role: "Operator".into(),
            cert_pem: "-----BEGIN CERTIFICATE-----\n...".into(),
            key_pem: "-----BEGIN PRIVATE KEY-----\n...".into(),
            ca_cert_pem: "-----BEGIN CERTIFICATE-----\n...".into(),
        };
        let envelope = accepted.seal_credential(&credential).unwrap();
        let opened = established.open_credential(&envelope).unwrap();
        assert_eq!(opened, credential);
        // The reverse direction is the new device's request channel.
        let request = established.session.c2s.seal(b"ready").unwrap();
        assert_eq!(accepted.session.c2s.open(&request).unwrap(), b"ready");
    }

    #[test]
    fn a_credential_for_another_device_is_refused() {
        let (_trusted, mut accepted, mut established) = ceremony();
        let credential = DeviceCredential {
            device_id: "tablet".into(),
            serial: "serial-1".into(),
            role: "Operator".into(),
            cert_pem: "cert".into(),
            key_pem: "key".into(),
            ca_cert_pem: "ca".into(),
        };
        let envelope = accepted.seal_credential(&credential).unwrap();
        assert!(matches!(
            established.open_credential(&envelope),
            Err(PairingError::Credential(_))
        ));
    }

    #[test]
    fn a_request_with_a_broken_confirmation_is_refused() {
        let mut trusted = offer();
        let invite = trusted.invite().clone();
        let joiner = PairingJoiner::begin(&invite, "phone", now()).unwrap();
        let mut request = joiner.request().clone();
        request.confirmation = URL_SAFE_NO_PAD.encode([0u8; 32]);
        assert!(matches!(
            trusted.accept(&request, now()),
            Err(PairingError::BadConfirmation)
        ));
    }

    #[test]
    fn a_request_with_a_broken_device_proof_is_refused() {
        let mut trusted = offer();
        let invite = trusted.invite().clone();
        let joiner = PairingJoiner::begin(&invite, "phone", now()).unwrap();
        let mut request = joiner.request().clone();
        request.device_signature = URL_SAFE_NO_PAD.encode([0u8; 64]);
        assert!(matches!(
            trusted.accept(&request, now()),
            Err(PairingError::BadDeviceProof)
        ));
    }

    #[test]
    fn a_wrong_sas_is_refused_even_when_the_mac_is_valid() {
        let mut trusted = offer();
        let invite = trusted.invite().clone();
        let joiner = PairingJoiner::begin(&invite, "phone", now()).unwrap();
        let mut accepted = trusted.accept(joiner.request(), now()).unwrap();
        accepted.acceptance.sas = "000 000".into();
        assert!(matches!(
            joiner.complete(&accepted.acceptance),
            Err(PairingError::MismatchedProof)
        ));
    }

    #[test]
    fn an_offer_is_single_use() {
        let mut trusted = offer();
        let invite = trusted.invite().clone();
        let joiner = PairingJoiner::begin(&invite, "phone", now()).unwrap();
        let request = joiner.request().clone();
        assert!(trusted.accept(&request, now()).is_ok());
        assert!(matches!(
            trusted.accept(&request, now()),
            Err(PairingError::Closed)
        ));
    }

    #[test]
    fn an_expired_offer_is_refused_on_both_sides() {
        let expired = PairingOffer::create(
            "https://relay.goble.dev",
            "desktop",
            now() - chrono::Duration::seconds(300),
            std::time::Duration::from_secs(120),
        )
        .unwrap();
        assert!(matches!(
            PairingJoiner::begin(expired.invite(), "phone", now()),
            Err(PairingError::Expired)
        ));

        // Build a request while the offer is fresh, then let it expire before
        // the trusted device accepts it.
        let mut fresh = PairingOffer::create(
            "https://relay.goble.dev",
            "desktop",
            now(),
            std::time::Duration::from_secs(120),
        )
        .unwrap();
        let joiner = PairingJoiner::begin(fresh.invite(), "phone", now()).unwrap();
        let request = joiner.request().clone();
        assert!(matches!(
            fresh.accept(&request, now() + chrono::Duration::seconds(121)),
            Err(PairingError::Expired)
        ));
    }

    #[test]
    fn a_request_for_another_offer_is_refused() {
        let mut trusted = offer();
        let other = offer();
        let joiner = PairingJoiner::begin(other.invite(), "phone", now()).unwrap();
        assert!(matches!(
            trusted.accept(joiner.request(), now()),
            Err(PairingError::WrongOffer)
        ));
    }

    #[test]
    fn the_offer_book_claims_once_and_purges() {
        let trusted = offer();
        let invite = trusted.invite().clone();
        let mut book = OfferBook::new();
        book.reserve(&invite).unwrap();
        let hash = invite.secret_hash().unwrap();
        assert_eq!(book.len(), 1);
        assert!(matches!(
            book.claim(invite.offer_id(), &[0u8; 32], now()),
            Err(PairingError::MismatchedProof)
        ));
        book.claim(invite.offer_id(), &hash, now()).unwrap();
        assert!(matches!(
            book.claim(invite.offer_id(), &hash, now()),
            Err(PairingError::Used)
        ));
        assert_eq!(book.purge(now()), 1);
        assert!(book.is_empty());
    }

    #[test]
    fn an_expired_offer_book_entry_is_refused_and_purged() {
        let invite = offer().invite().clone();
        let mut book = OfferBook::new();
        book.reserve(&invite).unwrap();
        let later = now() + chrono::Duration::seconds(300);
        assert!(matches!(
            book.claim(invite.offer_id(), &invite.secret_hash().unwrap(), later),
            Err(PairingError::Expired)
        ));
        assert_eq!(book.purge(later), 1);
    }

    #[test]
    fn reserving_the_same_offer_id_twice_is_refused() {
        let invite = offer().invite().clone();
        let mut book = OfferBook::new();
        book.reserve(&invite).unwrap();
        assert!(matches!(book.reserve(&invite), Err(PairingError::Used)));
    }

    #[test]
    fn the_joining_key_is_kept_for_the_keystore() {
        let (_trusted, _accepted, established) = ceremony();
        let key = Ed25519KeyPair::from_pkcs8(&established.device_key_pkcs8)
            .expect("a storable device key");
        assert_eq!(key.public_key().as_ref().len(), 32);
    }

    #[test]
    fn percent_encoding_handles_reserved_characters() {
        let value = "a b/c?d=e&f%g~";
        assert_eq!(decode_component(&encode_component(value)).unwrap(), value);
    }
}
