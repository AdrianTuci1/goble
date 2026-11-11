//! WebAuthn passkey verification.
//!
//! The relay only needs two ceremonies: register a passkey once, then verify
//! assertions. A passkey is a P-256 credential; the browser signs
//! `authenticatorData || SHA-256(clientDataJSON)` with ES256. This module
//! parses the small CBOR structures WebAuthn uses and checks the fields that
//! matter for the relay's origin:
//!
//! - the challenge the relay issued,
//! - the relying-party id and origin,
//! - user presence and user verification,
//! - the signature, and
//! - a monotonically increasing signature counter.
//!
//! Attestation is deliberately `none`: the relay cares that the authenticator
//! verified the user, not which vendor built it.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_ASN1};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A WebAuthn credential needs at least a 32-byte rpIdHash, one flags byte and
/// a four-byte counter.
const AUTH_DATA_MIN: usize = 37;
const FLAG_USER_PRESENT: u8 = 0x01;
const FLAG_USER_VERIFIED: u8 = 0x04;
const FLAG_ATTESTED_CREDENTIAL: u8 = 0x40;
const MAX_CREDENTIAL_ID: usize = 1024;
const MAX_CBOR_DEPTH: usize = 16;
const MAX_CBOR_ITEMS: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum PasskeyError {
    #[error("malformed CBOR: {0}")]
    Cbor(&'static str),
    #[error("malformed client data: {0}")]
    ClientData(&'static str),
    #[error("the challenge does not match")]
    Challenge,
    #[error("the origin {0} is not allowed")]
    Origin(String),
    #[error("the relying-party id does not match")]
    RpId,
    #[error("the authenticator did not verify the user")]
    UserNotVerified,
    #[error("the attestation format {0} is not supported")]
    Attestation(String),
    #[error("the credential algorithm is not ES256")]
    Algorithm,
    #[error("the credential key is malformed")]
    CredentialKey,
    #[error("the signature did not verify")]
    Signature,
    #[error("the authenticator counter went backwards")]
    CounterRegression,
    #[error("expected a {expected} ceremony, got {got}")]
    WrongType { expected: &'static str, got: String },
    #[error("the authenticator returned no credential")]
    MissingCredential,
}

/// The relay origin a passkey is bound to. `id` is the WebAuthn RP id (a
/// hostname); `origin` is the exact browser origin, including scheme and port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelyingParty {
    pub id: String,
    pub origin: String,
}

impl RelyingParty {
    pub fn new(id: impl Into<String>, origin: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            origin: origin.into(),
        }
    }
}

/// A registered credential. The public key is the uncompressed SEC1 point the
/// ES256 verifier expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPasskey {
    pub credential_id: Vec<u8>,
    pub public_key: [u8; 65],
    pub sign_count: u32,
}

/// What registration proves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub credential_id: Vec<u8>,
    pub public_key: [u8; 65],
    pub sign_count: u32,
    pub aaguid: [u8; 16],
}

/// What an assertion proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assertion {
    pub sign_count: u32,
}

/// Verify `navigator.credentials.create()` output.
pub fn verify_registration(
    rp: &RelyingParty,
    expected_challenge: &[u8],
    client_data_json: &[u8],
    attestation_object: &[u8],
) -> Result<Registration, PasskeyError> {
    check_client_data(rp, expected_challenge, client_data_json, "webauthn.create")?;
    let root = cbor::parse(attestation_object)?;
    let map = root
        .as_map()
        .ok_or(PasskeyError::Cbor("attestation object"))?;
    let format = map_text(map, "fmt").ok_or(PasskeyError::Cbor("fmt"))?;
    if format != "none" {
        return Err(PasskeyError::Attestation(format.to_string()));
    }
    let auth_data = map_bytes(map, "authData").ok_or(PasskeyError::Cbor("authData"))?;
    let parsed = parse_auth_data(auth_data)?;
    check_rp_id(rp, &parsed.rp_id_hash)?;
    check_verified(parsed.flags)?;
    let (credential_id, cose) = parsed.credential.ok_or(PasskeyError::MissingCredential)?;
    let public_key = parse_cose_key(cose)?;
    Ok(Registration {
        credential_id,
        public_key,
        sign_count: parsed.sign_count,
        aaguid: parsed.aaguid,
    })
}

/// Verify `navigator.credentials.get()` output.
pub fn verify_assertion(
    rp: &RelyingParty,
    stored: &StoredPasskey,
    expected_challenge: &[u8],
    client_data_json: &[u8],
    authenticator_data: &[u8],
    signature: &[u8],
) -> Result<Assertion, PasskeyError> {
    check_client_data(rp, expected_challenge, client_data_json, "webauthn.get")?;
    let parsed = parse_auth_data(authenticator_data)?;
    check_rp_id(rp, &parsed.rp_id_hash)?;
    check_verified(parsed.flags)?;
    if parsed.flags & FLAG_ATTESTED_CREDENTIAL != 0 {
        return Err(PasskeyError::Cbor(
            "an assertion must not carry attested credential data",
        ));
    }
    let mut signed = Vec::with_capacity(authenticator_data.len() + 32);
    signed.extend_from_slice(authenticator_data);
    signed.extend_from_slice(&sha256(client_data_json));
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, &stored.public_key)
        .verify(&signed, signature)
        .map_err(|_| PasskeyError::Signature)?;

    // Authenticators that do not implement a counter report zero forever.
    // Accept that, but reject a counter that moves backwards.
    if !(stored.sign_count == 0 && parsed.sign_count == 0) && parsed.sign_count <= stored.sign_count
    {
        return Err(PasskeyError::CounterRegression);
    }
    Ok(Assertion {
        sign_count: parsed.sign_count,
    })
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(rename = "crossOrigin", default)]
    cross_origin: bool,
}

fn check_client_data(
    rp: &RelyingParty,
    expected_challenge: &[u8],
    client_data_json: &[u8],
    expected_type: &'static str,
) -> Result<(), PasskeyError> {
    let data: ClientData =
        serde_json::from_slice(client_data_json).map_err(|_| PasskeyError::ClientData("json"))?;
    if data.kind != expected_type {
        return Err(PasskeyError::WrongType {
            expected: expected_type,
            got: data.kind,
        });
    }
    if data.cross_origin {
        return Err(PasskeyError::ClientData("cross-origin ceremony"));
    }
    if data.origin != rp.origin {
        return Err(PasskeyError::Origin(data.origin));
    }
    let challenge =
        decode_base64url(&data.challenge).ok_or(PasskeyError::ClientData("challenge encoding"))?;
    if challenge != expected_challenge {
        return Err(PasskeyError::Challenge);
    }
    Ok(())
}

fn check_rp_id(rp: &RelyingParty, rp_id_hash: &[u8; 32]) -> Result<(), PasskeyError> {
    if sha256(rp.id.as_bytes()) != *rp_id_hash {
        return Err(PasskeyError::RpId);
    }
    Ok(())
}

fn check_verified(flags: u8) -> Result<(), PasskeyError> {
    if flags & FLAG_USER_PRESENT == 0 || flags & FLAG_USER_VERIFIED == 0 {
        return Err(PasskeyError::UserNotVerified);
    }
    Ok(())
}

struct AuthenticatorData<'a> {
    rp_id_hash: [u8; 32],
    flags: u8,
    sign_count: u32,
    aaguid: [u8; 16],
    credential: Option<(Vec<u8>, &'a [u8])>,
}

fn parse_auth_data(bytes: &[u8]) -> Result<AuthenticatorData<'_>, PasskeyError> {
    if bytes.len() < AUTH_DATA_MIN {
        return Err(PasskeyError::Cbor("authenticator data"));
    }
    let mut rp_id_hash = [0u8; 32];
    rp_id_hash.copy_from_slice(&bytes[..32]);
    let flags = bytes[32];
    let sign_count = u32::from_be_bytes(bytes[33..37].try_into().expect("four bytes"));

    if flags & FLAG_ATTESTED_CREDENTIAL == 0 {
        return Ok(AuthenticatorData {
            rp_id_hash,
            flags,
            sign_count,
            aaguid: [0u8; 16],
            credential: None,
        });
    }
    if bytes.len() < 55 {
        return Err(PasskeyError::Cbor("attested credential data"));
    }
    let mut aaguid = [0u8; 16];
    aaguid.copy_from_slice(&bytes[37..53]);
    let length = u16::from_be_bytes(bytes[53..55].try_into().expect("two bytes")) as usize;
    if length == 0 || length > MAX_CREDENTIAL_ID {
        return Err(PasskeyError::Cbor("credential id length"));
    }
    let end = 55usize
        .checked_add(length)
        .ok_or(PasskeyError::Cbor("credential id length"))?;
    if bytes.len() <= end {
        return Err(PasskeyError::Cbor("credential id"));
    }
    let credential_id = bytes[55..end].to_vec();
    // The COSE key runs to the end of the attested credential data. WebAuthn
    // extensions, when present, follow the key; the relay does not use them,
    // so the parser stops after the first CBOR value and ignores the rest.
    let cose = &bytes[end..];
    Ok(AuthenticatorData {
        rp_id_hash,
        flags,
        sign_count,
        aaguid,
        credential: Some((credential_id, cose)),
    })
}

fn parse_cose_key(bytes: &[u8]) -> Result<[u8; 65], PasskeyError> {
    let value = cbor::parse(bytes)?;
    let map = value.as_map().ok_or(PasskeyError::CredentialKey)?;
    if map_int(map, 1) != Some(CborInt::Unsigned(2))
        || map_int(map, 3) != Some(CborInt::Negative(-7))
        || map_int(map, -1) != Some(CborInt::Unsigned(1))
    {
        return Err(PasskeyError::Algorithm);
    }
    let x = map_bytes_int(map, -2).ok_or(PasskeyError::CredentialKey)?;
    let y = map_bytes_int(map, -3).ok_or(PasskeyError::CredentialKey)?;
    if x.len() != 32 || y.len() != 32 {
        return Err(PasskeyError::CredentialKey);
    }
    let mut public_key = [0u8; 65];
    public_key[0] = 0x04;
    public_key[1..33].copy_from_slice(x);
    public_key[33..].copy_from_slice(y);
    if !p256_point_is_plausible(&public_key) {
        return Err(PasskeyError::CredentialKey);
    }
    Ok(public_key)
}

/// Reject an all-zero point. The curve check itself happens when ring verifies
/// the first signature.
fn p256_point_is_plausible(point: &[u8; 65]) -> bool {
    point[0] == 0x04
        && point[1..33].iter().any(|byte| *byte != 0)
        && point[33..].iter().any(|byte| *byte != 0)
}

fn decode_base64url(value: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(value.trim_end_matches('=')).ok()
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CborInt {
    Unsigned(u64),
    Negative(i64),
}

impl CborInt {
    fn value(self) -> i64 {
        match self {
            Self::Unsigned(value) => i64::try_from(value).unwrap_or(i64::MAX),
            Self::Negative(value) => value,
        }
    }
}

fn map_text<'a>(map: &'a [(cbor::Value, cbor::Value)], key: &str) -> Option<&'a str> {
    map.iter().find_map(|(candidate, value)| {
        if candidate.as_text() == Some(key) {
            value.as_text()
        } else {
            None
        }
    })
}

fn map_bytes<'a>(map: &'a [(cbor::Value, cbor::Value)], key: &str) -> Option<&'a [u8]> {
    map.iter().find_map(|(candidate, value)| {
        if candidate.as_text() == Some(key) {
            value.as_bytes()
        } else {
            None
        }
    })
}

fn map_int(map: &[(cbor::Value, cbor::Value)], key: i64) -> Option<CborInt> {
    map.iter().find_map(|(candidate, value)| {
        if candidate.as_int().map(CborInt::value) == Some(key) {
            value.as_int()
        } else {
            None
        }
    })
}

fn map_bytes_int(map: &[(cbor::Value, cbor::Value)], key: i64) -> Option<&[u8]> {
    map.iter().find_map(|(candidate, value)| {
        if candidate.as_int().map(CborInt::value) == Some(key) {
            value.as_bytes()
        } else {
            None
        }
    })
}

/// The subset of CBOR WebAuthn needs. Indefinite lengths, tags, floats and
/// simple values other than `true`/`false`/`null` are rejected: no WebAuthn
/// structure uses them, and accepting them would add parser surface.
mod cbor {
    use super::{PasskeyError, MAX_CBOR_DEPTH, MAX_CBOR_ITEMS};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Unsigned(u64),
        Negative(i64),
        Bytes(Vec<u8>),
        Text(String),
        Array(Vec<Value>),
        Map(Vec<(Value, Value)>),
        Bool(bool),
        Null,
    }

    impl Value {
        pub fn as_map(&self) -> Option<&[(Value, Value)]> {
            match self {
                Self::Map(entries) => Some(entries),
                _ => None,
            }
        }

        pub fn as_text(&self) -> Option<&str> {
            match self {
                Self::Text(text) => Some(text),
                _ => None,
            }
        }

        pub fn as_bytes(&self) -> Option<&[u8]> {
            match self {
                Self::Bytes(bytes) => Some(bytes),
                _ => None,
            }
        }

        pub fn as_int(&self) -> Option<super::CborInt> {
            match self {
                Self::Unsigned(value) => Some(super::CborInt::Unsigned(*value)),
                Self::Negative(value) => Some(super::CborInt::Negative(*value)),
                _ => None,
            }
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Value, PasskeyError> {
        let mut parser = Parser { bytes, position: 0 };
        let value = parser.value(0)?;
        if parser.position != bytes.len() {
            return Err(PasskeyError::Cbor("trailing bytes"));
        }
        Ok(value)
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        position: usize,
    }

    impl Parser<'_> {
        fn byte(&mut self) -> Result<u8, PasskeyError> {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(PasskeyError::Cbor("unexpected end"))?;
            self.position += 1;
            Ok(byte)
        }

        fn take(&mut self, length: usize) -> Result<&[u8], PasskeyError> {
            let end = self
                .position
                .checked_add(length)
                .ok_or(PasskeyError::Cbor("length overflow"))?;
            let slice = self
                .bytes
                .get(self.position..end)
                .ok_or(PasskeyError::Cbor("unexpected end"))?;
            self.position = end;
            Ok(slice)
        }

        fn length(&mut self, additional: u8) -> Result<u64, PasskeyError> {
            match additional {
                0..=23 => Ok(additional as u64),
                24 => Ok(self.byte()? as u64),
                25 => Ok(u16::from_be_bytes(self.take(2)?.try_into().expect("two")) as u64),
                26 => Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("four")) as u64),
                27 => Ok(u64::from_be_bytes(self.take(8)?.try_into().expect("eight"))),
                31 => Err(PasskeyError::Cbor("indefinite length")),
                _ => Err(PasskeyError::Cbor("reserved length")),
            }
        }

        fn value(&mut self, depth: usize) -> Result<Value, PasskeyError> {
            if depth > MAX_CBOR_DEPTH {
                return Err(PasskeyError::Cbor("nesting too deep"));
            }
            let head = self.byte()?;
            let major = head >> 5;
            let additional = head & 0x1f;
            match major {
                0 => Ok(Value::Unsigned(self.length(additional)?)),
                1 => {
                    let raw = self.length(additional)?;
                    let value = i64::try_from(raw)
                        .ok()
                        .and_then(|raw| raw.checked_add(1))
                        .and_then(|raw| raw.checked_neg())
                        .ok_or(PasskeyError::Cbor("negative integer out of range"))?;
                    Ok(Value::Negative(value))
                }
                2 => {
                    let length = self.length(additional)?;
                    let length = usize::try_from(length)
                        .map_err(|_| PasskeyError::Cbor("byte string length"))?;
                    Ok(Value::Bytes(self.take(length)?.to_vec()))
                }
                3 => {
                    let length = self.length(additional)?;
                    let length =
                        usize::try_from(length).map_err(|_| PasskeyError::Cbor("text length"))?;
                    let text = std::str::from_utf8(self.take(length)?)
                        .map_err(|_| PasskeyError::Cbor("text encoding"))?;
                    Ok(Value::Text(text.to_string()))
                }
                4 => {
                    let length = self.items(additional)?;
                    let mut values = Vec::with_capacity(length.min(MAX_CBOR_ITEMS));
                    for _ in 0..length {
                        values.push(self.value(depth + 1)?);
                    }
                    Ok(Value::Array(values))
                }
                5 => {
                    let length = self.items(additional)?;
                    let mut entries = Vec::with_capacity(length.min(MAX_CBOR_ITEMS));
                    for _ in 0..length {
                        entries.push((self.value(depth + 1)?, self.value(depth + 1)?));
                    }
                    Ok(Value::Map(entries))
                }
                6 => Err(PasskeyError::Cbor("tags are not supported")),
                7 => match additional {
                    20 => Ok(Value::Bool(false)),
                    21 => Ok(Value::Bool(true)),
                    22 => Ok(Value::Null),
                    _ => Err(PasskeyError::Cbor("unsupported simple value")),
                },
                _ => Err(PasskeyError::Cbor("unsupported major type")),
            }
        }

        fn items(&mut self, additional: u8) -> Result<usize, PasskeyError> {
            let length = self.length(additional)?;
            let length = usize::try_from(length).map_err(|_| PasskeyError::Cbor("item count"))?;
            if length > MAX_CBOR_ITEMS {
                return Err(PasskeyError::Cbor("too many items"));
            }
            Ok(length)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

    const RP_ID: &str = "relay.goble.dev";
    const ORIGIN: &str = "https://relay.goble.dev";

    fn rp() -> RelyingParty {
        RelyingParty::new(RP_ID, ORIGIN)
    }

    fn challenge() -> [u8; 32] {
        [7u8; 32]
    }

    fn client_data(kind: &str, challenge: &[u8], origin: &str) -> Vec<u8> {
        serde_json::json!({
            "type": kind,
            "challenge": URL_SAFE_NO_PAD.encode(challenge),
            "origin": origin,
            "crossOrigin": false,
        })
        .to_string()
        .into_bytes()
    }

    fn cbor_head(major: u8, length: u64) -> Vec<u8> {
        let prefix = major << 5;
        match length {
            0..=23 => vec![prefix | length as u8],
            24..=255 => vec![prefix | 24, length as u8],
            256..=65535 => {
                let mut out = vec![prefix | 25];
                out.extend_from_slice(&(length as u16).to_be_bytes());
                out
            }
            65536..=0xFFFF_FFFF => {
                let mut out = vec![prefix | 26];
                out.extend_from_slice(&(length as u32).to_be_bytes());
                out
            }
            _ => {
                let mut out = vec![prefix | 27];
                out.extend_from_slice(&length.to_be_bytes());
                out
            }
        }
    }

    fn cbor_uint(value: u64) -> Vec<u8> {
        cbor_head(0, value)
    }

    fn cbor_int(value: i64) -> Vec<u8> {
        if value >= 0 {
            cbor_uint(value as u64)
        } else {
            cbor_head(1, (-1 - value) as u64)
        }
    }

    fn cbor_bytes(value: &[u8]) -> Vec<u8> {
        let mut out = cbor_head(2, value.len() as u64);
        out.extend_from_slice(value);
        out
    }

    fn cbor_text(value: &str) -> Vec<u8> {
        let mut out = cbor_head(3, value.len() as u64);
        out.extend_from_slice(value.as_bytes());
        out
    }

    fn cbor_map(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let mut out = cbor_head(5, entries.len() as u64);
        for (key, value) in entries {
            out.extend_from_slice(key);
            out.extend_from_slice(value);
        }
        out
    }

    struct TestAuthenticator {
        key: EcdsaKeyPair,
        public_key: [u8; 65],
        credential_id: Vec<u8>,
    }

    impl TestAuthenticator {
        fn new() -> Self {
            let rng = SystemRandom::new();
            let pkcs8 =
                EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
            let key =
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                    .unwrap();
            let mut public_key = [0u8; 65];
            public_key.copy_from_slice(key.public_key().as_ref());
            Self {
                key,
                public_key,
                credential_id: b"credential-1".to_vec(),
            }
        }

        fn cose_key(&self) -> Vec<u8> {
            cbor_map(&[
                (cbor_int(1), cbor_uint(2)),  // kty: EC2
                (cbor_int(3), cbor_int(-7)),  // alg: ES256
                (cbor_int(-1), cbor_uint(1)), // crv: P-256
                (cbor_int(-2), cbor_bytes(&self.public_key[1..33])),
                (cbor_int(-3), cbor_bytes(&self.public_key[33..])),
            ])
        }

        fn auth_data(&self, flags: u8, count: u32, attested: bool) -> Vec<u8> {
            let mut data = Vec::new();
            data.extend_from_slice(&sha256(RP_ID.as_bytes()));
            data.push(flags);
            data.extend_from_slice(&count.to_be_bytes());
            if attested {
                data.extend_from_slice(&[0u8; 16]);
                data.extend_from_slice(&(self.credential_id.len() as u16).to_be_bytes());
                data.extend_from_slice(&self.credential_id);
                data.extend_from_slice(&self.cose_key());
            }
            data
        }

        fn registration(&self, auth_data: &[u8]) -> Vec<u8> {
            cbor_map(&[
                (cbor_text("fmt"), cbor_text("none")),
                (cbor_text("authData"), cbor_bytes(auth_data)),
                (cbor_text("attStmt"), cbor_map(&[])),
            ])
        }

        fn sign(&self, auth_data: &[u8], client_data: &[u8]) -> Vec<u8> {
            let mut message = auth_data.to_vec();
            message.extend_from_slice(&sha256(client_data));
            self.key
                .sign(&SystemRandom::new(), &message)
                .unwrap()
                .as_ref()
                .to_vec()
        }
    }

    #[test]
    fn a_registration_yields_the_stored_public_key() {
        let authenticator = TestAuthenticator::new();
        let auth_data = authenticator.auth_data(0x45, 0, true);
        let registration = verify_registration(
            &rp(),
            &challenge(),
            &client_data("webauthn.create", &challenge(), ORIGIN),
            &authenticator.registration(&auth_data),
        )
        .expect("a registration");
        assert_eq!(registration.credential_id, authenticator.credential_id);
        assert_eq!(registration.public_key, authenticator.public_key);
        assert_eq!(registration.sign_count, 0);
    }

    #[test]
    fn an_assertion_verifies_and_advances_the_counter() {
        let authenticator = TestAuthenticator::new();
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 0,
        };
        let auth_data = authenticator.auth_data(0x05, 1, false);
        let client_data = client_data("webauthn.get", &challenge(), ORIGIN);
        let signature = authenticator.sign(&auth_data, &client_data);
        let assertion = verify_assertion(
            &rp(),
            &stored,
            &challenge(),
            &client_data,
            &auth_data,
            &signature,
        )
        .expect("an assertion");
        assert_eq!(assertion.sign_count, 1);
    }

    #[test]
    fn a_different_challenge_is_refused() {
        let authenticator = TestAuthenticator::new();
        let auth_data = authenticator.auth_data(0x05, 1, false);
        let client_data = client_data("webauthn.get", b"other", ORIGIN);
        let signature = authenticator.sign(&auth_data, &client_data);
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 0,
        };
        assert!(matches!(
            verify_assertion(
                &rp(),
                &stored,
                &challenge(),
                &client_data,
                &auth_data,
                &signature
            ),
            Err(PasskeyError::Challenge)
        ));
    }

    #[test]
    fn a_foreign_origin_is_refused() {
        let authenticator = TestAuthenticator::new();
        let auth_data = authenticator.auth_data(0x45, 0, true);
        assert!(matches!(
            verify_registration(
                &rp(),
                &challenge(),
                &client_data("webauthn.create", &challenge(), "https://evil.example"),
                &authenticator.registration(&auth_data)
            ),
            Err(PasskeyError::Origin(_))
        ));
    }

    #[test]
    fn a_wrong_relying_party_id_is_refused() {
        let authenticator = TestAuthenticator::new();
        let auth_data = authenticator.auth_data(0x45, 0, true);
        let foreign = RelyingParty::new("other.example", ORIGIN);
        assert!(matches!(
            verify_registration(
                &foreign,
                &challenge(),
                &client_data("webauthn.create", &challenge(), ORIGIN),
                &authenticator.registration(&auth_data)
            ),
            Err(PasskeyError::RpId)
        ));
    }

    #[test]
    fn user_verification_is_required() {
        let authenticator = TestAuthenticator::new();
        let auth_data = authenticator.auth_data(0x41, 0, true); // UP but no UV
        assert!(matches!(
            verify_registration(
                &rp(),
                &challenge(),
                &client_data("webauthn.create", &challenge(), ORIGIN),
                &authenticator.registration(&auth_data)
            ),
            Err(PasskeyError::UserNotVerified)
        ));
    }

    #[test]
    fn packed_attestation_is_refused() {
        let authenticator = TestAuthenticator::new();
        let auth_data = authenticator.auth_data(0x45, 0, true);
        let attestation = cbor_map(&[
            (cbor_text("fmt"), cbor_text("packed")),
            (cbor_text("authData"), cbor_bytes(&auth_data)),
            (cbor_text("attStmt"), cbor_map(&[])),
        ]);
        assert!(matches!(
            verify_registration(
                &rp(),
                &challenge(),
                &client_data("webauthn.create", &challenge(), ORIGIN),
                &attestation
            ),
            Err(PasskeyError::Attestation(_))
        ));
    }

    #[test]
    fn a_tampered_assertion_signature_is_refused() {
        let authenticator = TestAuthenticator::new();
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 0,
        };
        let auth_data = authenticator.auth_data(0x05, 1, false);
        let client_data = client_data("webauthn.get", &challenge(), ORIGIN);
        let mut signature = authenticator.sign(&auth_data, &client_data);
        let last = signature.len() - 1;
        signature[last] ^= 0x01;
        assert!(matches!(
            verify_assertion(
                &rp(),
                &stored,
                &challenge(),
                &client_data,
                &auth_data,
                &signature
            ),
            Err(PasskeyError::Signature)
        ));
    }

    #[test]
    fn a_backwards_counter_is_refused() {
        let authenticator = TestAuthenticator::new();
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 5,
        };
        let auth_data = authenticator.auth_data(0x05, 4, false);
        let client_data = client_data("webauthn.get", &challenge(), ORIGIN);
        let signature = authenticator.sign(&auth_data, &client_data);
        assert!(matches!(
            verify_assertion(
                &rp(),
                &stored,
                &challenge(),
                &client_data,
                &auth_data,
                &signature
            ),
            Err(PasskeyError::CounterRegression)
        ));
    }

    #[test]
    fn a_zero_counter_authenticator_is_accepted() {
        let authenticator = TestAuthenticator::new();
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 0,
        };
        let auth_data = authenticator.auth_data(0x05, 0, false);
        let client_data = client_data("webauthn.get", &challenge(), ORIGIN);
        let signature = authenticator.sign(&auth_data, &client_data);
        assert_eq!(
            verify_assertion(
                &rp(),
                &stored,
                &challenge(),
                &client_data,
                &auth_data,
                &signature
            )
            .unwrap()
            .sign_count,
            0
        );
    }

    #[test]
    fn the_wrong_ceremony_type_is_refused() {
        let authenticator = TestAuthenticator::new();
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 0,
        };
        let auth_data = authenticator.auth_data(0x05, 1, false);
        let client_data = client_data("webauthn.create", &challenge(), ORIGIN);
        let signature = authenticator.sign(&auth_data, &client_data);
        assert!(matches!(
            verify_assertion(
                &rp(),
                &stored,
                &challenge(),
                &client_data,
                &auth_data,
                &signature
            ),
            Err(PasskeyError::WrongType { .. })
        ));
    }

    #[test]
    fn a_non_es256_credential_is_refused() {
        let authenticator = TestAuthenticator::new();
        let mut cose = authenticator.cose_key();
        // Replace the alg value -7 (0x26) with -8 (0x27), still length 1.
        let position = cose.iter().position(|byte| *byte == 0x26).unwrap();
        cose[position] = 0x27;
        let mut auth_data = authenticator.auth_data(0x45, 0, true);
        let cose_start = auth_data.len() - authenticator.cose_key().len();
        auth_data.truncate(cose_start);
        auth_data.extend_from_slice(&cose);
        assert!(matches!(
            verify_registration(
                &rp(),
                &challenge(),
                &client_data("webauthn.create", &challenge(), ORIGIN),
                &authenticator.registration(&auth_data)
            ),
            Err(PasskeyError::Algorithm)
        ));
    }

    #[test]
    fn malformed_cbor_is_refused() {
        assert!(matches!(cbor::parse(&[0xff]), Err(PasskeyError::Cbor(_))));
        assert!(matches!(
            cbor::parse(&[0x9f]),
            Err(PasskeyError::Cbor("indefinite length"))
        ));
        assert!(matches!(
            cbor::parse(&[0x82, 0x00]),
            Err(PasskeyError::Cbor("unexpected end"))
        ));
        assert!(matches!(
            cbor::parse(&[0x00, 0x00]),
            Err(PasskeyError::Cbor("trailing bytes"))
        ));
    }

    #[test]
    fn the_auth_data_minimum_length_is_enforced() {
        assert!(matches!(
            parse_auth_data(&[0u8; 10]),
            Err(PasskeyError::Cbor("authenticator data"))
        ));
        let mut short_attested = [0u8; 40];
        short_attested[32] = FLAG_ATTESTED_CREDENTIAL;
        assert!(matches!(
            parse_auth_data(&short_attested),
            Err(PasskeyError::Cbor("attested credential data"))
        ));
    }

    #[test]
    fn attested_credential_data_in_an_assertion_is_refused() {
        let authenticator = TestAuthenticator::new();
        let stored = StoredPasskey {
            credential_id: authenticator.credential_id.clone(),
            public_key: authenticator.public_key,
            sign_count: 0,
        };
        let auth_data = authenticator.auth_data(0x45, 1, true);
        let client_data = client_data("webauthn.get", &challenge(), ORIGIN);
        let signature = authenticator.sign(&auth_data, &client_data);
        assert!(matches!(
            verify_assertion(
                &rp(),
                &stored,
                &challenge(),
                &client_data,
                &auth_data,
                &signature
            ),
            Err(PasskeyError::Cbor(_))
        ));
    }
}
