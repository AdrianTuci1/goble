//! Trigger intake: verify the sender before parsing the event.
//!
//! Every webhook carries a signature over its exact body. The hub checks it
//! first and only then decodes JSON for routing. A trigger binds to one
//! workflow; fan-out belongs in the workflow, not in the webhook.

use ring::hmac;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde_json::Value;

/// How long a Slack request may be old or ahead of the hub. Slack recommends
/// five minutes; a tighter window makes clock drift a support problem.
pub const SLACK_TOLERANCE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    GitHub,
    Slack,
    Linear,
    Discord,
    Http,
    Schedule,
    Mcp,
}

impl TriggerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Slack => "slack",
            Self::Linear => "linear",
            Self::Discord => "discord",
            Self::Http => "http",
            Self::Schedule => "schedule",
            Self::Mcp => "mcp",
        }
    }
}

/// A verified webhook. Only construct this through one of the `verify_*`
/// functions; the fields are the routing inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedWebhook {
    pub source: TriggerSource,
    pub delivery_id: String,
    pub event: String,
    pub payload: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum TriggerError {
    #[error("the signature header is missing")]
    MissingSignature,
    #[error("the signature header is malformed")]
    MalformedSignature,
    #[error("the signature does not match the body")]
    InvalidSignature,
    #[error("the signature is outside the allowed time window")]
    StaleTimestamp,
    #[error("the bearer token is not valid")]
    InvalidToken,
    #[error("the payload is not valid JSON: {0}")]
    Payload(#[from] serde_json::Error),
    #[error("the delivery id is missing")]
    MissingDeliveryId,
    #[error("the event name is missing")]
    MissingEvent,
}

/// GitHub sends `X-Hub-Signature-256: sha256=<hex>`.
pub fn verify_github(
    secret: &[u8],
    body: &[u8],
    signature: &str,
    delivery_id: &str,
    event: &str,
) -> Result<VerifiedWebhook, TriggerError> {
    let expected = signature
        .strip_prefix("sha256=")
        .ok_or(TriggerError::MalformedSignature)?;
    let expected = decode_hex(expected).ok_or(TriggerError::MalformedSignature)?;
    verify_hmac(secret, body, &expected)?;
    verified(TriggerSource::GitHub, delivery_id, event, body)
}

/// Linear sends `Linear-Signature: <hex hmac>`.
pub fn verify_linear(
    secret: &[u8],
    body: &[u8],
    signature: &str,
    delivery_id: &str,
    event: &str,
) -> Result<VerifiedWebhook, TriggerError> {
    let expected = decode_hex(signature).ok_or(TriggerError::MalformedSignature)?;
    verify_hmac(secret, body, &expected)?;
    verified(TriggerSource::Linear, delivery_id, event, body)
}

/// Slack signs `v0:<timestamp>:<body>` with the signing secret and sends
/// `X-Slack-Signature: v0=<hex>`.
pub fn verify_slack(
    signing_secret: &[u8],
    timestamp: &str,
    body: &[u8],
    signature: &str,
    now: i64,
    delivery_id: &str,
    event: &str,
) -> Result<VerifiedWebhook, TriggerError> {
    let timestamp_value: i64 = timestamp
        .parse()
        .map_err(|_| TriggerError::MalformedSignature)?;
    if (now - timestamp_value).abs() > SLACK_TOLERANCE_SECS {
        return Err(TriggerError::StaleTimestamp);
    }
    let expected = signature
        .strip_prefix("v0=")
        .ok_or(TriggerError::MalformedSignature)?;
    let expected = decode_hex(expected).ok_or(TriggerError::MalformedSignature)?;
    let mut message = Vec::with_capacity(3 + timestamp.len() + 1 + body.len());
    message.extend_from_slice(b"v0:");
    message.extend_from_slice(timestamp.as_bytes());
    message.push(b':');
    message.extend_from_slice(body);
    verify_hmac(signing_secret, &message, &expected)?;
    verified(TriggerSource::Slack, delivery_id, event, body)
}

/// Discord signs `timestamp || body` with an Ed25519 app key. The public key is
/// a 32-byte hex string from the application's developer page.
pub fn verify_discord(
    public_key_hex: &str,
    timestamp: &str,
    body: &[u8],
    signature_hex: &str,
    delivery_id: &str,
    event: &str,
    now: i64,
) -> Result<VerifiedWebhook, TriggerError> {
    let timestamp_value: i64 = timestamp
        .parse()
        .map_err(|_| TriggerError::MalformedSignature)?;
    if (now - timestamp_value).abs() > SLACK_TOLERANCE_SECS {
        return Err(TriggerError::StaleTimestamp);
    }
    let public_key = decode_hex(public_key_hex).ok_or(TriggerError::MalformedSignature)?;
    let signature = decode_hex(signature_hex).ok_or(TriggerError::MalformedSignature)?;
    if public_key.len() != 32 || signature.len() != 64 {
        return Err(TriggerError::MalformedSignature);
    }
    let mut message = Vec::with_capacity(timestamp.len() + body.len());
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body);
    UnparsedPublicKey::new(&ED25519, &public_key)
        .verify(&message, &signature)
        .map_err(|_| TriggerError::InvalidSignature)?;
    verified(TriggerSource::Discord, delivery_id, event, body)
}

/// A scripted run or a CI job presents a scoped bearer token.
pub fn verify_http_token(presented: &str, expected: &str) -> Result<(), TriggerError> {
    let presented = presented
        .strip_prefix("Bearer ")
        .or_else(|| presented.strip_prefix("bearer "))
        .ok_or(TriggerError::InvalidToken)?;
    if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(TriggerError::InvalidToken)
    }
}

fn verify_hmac(secret: &[u8], message: &[u8], expected: &[u8]) -> Result<(), TriggerError> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::verify(&key, message, expected).map_err(|_| TriggerError::InvalidSignature)
}

fn verified(
    source: TriggerSource,
    delivery_id: &str,
    event: &str,
    body: &[u8],
) -> Result<VerifiedWebhook, TriggerError> {
    if delivery_id.trim().is_empty() {
        return Err(TriggerError::MissingDeliveryId);
    }
    if event.trim().is_empty() {
        return Err(TriggerError::MissingEvent);
    }
    Ok(VerifiedWebhook {
        source,
        delivery_id: delivery_id.to_string(),
        event: event.to_string(),
        payload: serde_json::from_slice(body)?,
    })
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    if value.len() % 2 != 0 {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn hmac_hex(secret: &[u8], message: &[u8]) -> String {
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
        hmac::sign(&key, message)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn body() -> Vec<u8> {
        br#"{"action":"opened","issue":{"number":1}}"#.to_vec()
    }

    #[test]
    fn a_github_delivery_verifies_and_parses() {
        let signature = format!("sha256={}", hmac_hex(b"secret", &body()));
        let event = verify_github(b"secret", &body(), &signature, "delivery-1", "issues")
            .expect("a verified delivery");
        assert_eq!(event.source, TriggerSource::GitHub);
        assert_eq!(event.delivery_id, "delivery-1");
        assert_eq!(event.event, "issues");
        assert_eq!(event.payload["action"], "opened");
    }

    #[test]
    fn a_github_delivery_with_the_wrong_secret_is_refused() {
        let signature = format!("sha256={}", hmac_hex(b"wrong", &body()));
        assert!(matches!(
            verify_github(b"secret", &body(), &signature, "delivery-1", "issues"),
            Err(TriggerError::InvalidSignature)
        ));
    }

    #[test]
    fn a_tampered_github_body_is_refused() {
        let signature = format!("sha256={}", hmac_hex(b"secret", &body()));
        let mut tampered = body();
        tampered[0] = b'[';
        assert!(matches!(
            verify_github(b"secret", &tampered, &signature, "delivery-1", "issues"),
            Err(TriggerError::InvalidSignature)
        ));
    }

    #[test]
    fn malformed_github_headers_are_refused() {
        assert!(matches!(
            verify_github(b"secret", &body(), "deadbeef", "delivery-1", "issues"),
            Err(TriggerError::MalformedSignature)
        ));
        assert!(matches!(
            verify_github(b"secret", &body(), "sha256=xyz", "delivery-1", "issues"),
            Err(TriggerError::MalformedSignature)
        ));
    }

    #[test]
    fn missing_delivery_id_or_event_is_refused_after_the_signature() {
        let signature = format!("sha256={}", hmac_hex(b"secret", &body()));
        assert!(matches!(
            verify_github(b"secret", &body(), &signature, " ", "issues"),
            Err(TriggerError::MissingDeliveryId)
        ));
        assert!(matches!(
            verify_github(b"secret", &body(), &signature, "delivery-1", ""),
            Err(TriggerError::MissingEvent)
        ));
    }

    #[test]
    fn invalid_json_after_a_valid_signature_is_a_payload_error() {
        let bad = b"not json".to_vec();
        let signature = format!("sha256={}", hmac_hex(b"secret", &bad));
        assert!(matches!(
            verify_github(b"secret", &bad, &signature, "delivery-1", "issues"),
            Err(TriggerError::Payload(_))
        ));
    }

    #[test]
    fn a_linear_delivery_verifies() {
        let signature = hmac_hex(b"linear-secret", &body());
        let event = verify_linear(
            b"linear-secret",
            &body(),
            &signature,
            "delivery-2",
            "issue.update",
        )
        .expect("a verified delivery");
        assert_eq!(event.source, TriggerSource::Linear);
    }

    #[test]
    fn a_slack_delivery_verifies_and_checks_the_clock() {
        let timestamp = "1700000000";
        let message = format!("v0:{timestamp}:{}", String::from_utf8_lossy(&body()));
        let signature = format!("v0={}", hmac_hex(b"slack-secret", message.as_bytes()));
        let event = verify_slack(
            b"slack-secret",
            timestamp,
            &body(),
            &signature,
            1_700_000_100,
            "delivery-3",
            "app_mention",
        )
        .expect("a verified delivery");
        assert_eq!(event.source, TriggerSource::Slack);

        assert!(matches!(
            verify_slack(
                b"slack-secret",
                timestamp,
                &body(),
                &signature,
                1_700_000_000 + SLACK_TOLERANCE_SECS + 1,
                "delivery-3",
                "app_mention"
            ),
            Err(TriggerError::StaleTimestamp)
        ));
    }

    #[test]
    fn a_slack_delivery_with_the_wrong_signature_is_refused() {
        let timestamp = "1700000000";
        let signature = format!("v0={}", hmac_hex(b"wrong", b"v0:1700000000:{}"));
        assert!(matches!(
            verify_slack(
                b"slack-secret",
                timestamp,
                &body(),
                &signature,
                1_700_000_000,
                "delivery-3",
                "app_mention"
            ),
            Err(TriggerError::InvalidSignature)
        ));
    }

    #[test]
    fn a_discord_interaction_verifies() {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let timestamp = "1700000000";
        let mut message = timestamp.as_bytes().to_vec();
        message.extend_from_slice(&body());
        let signature = key.sign(&message);
        let public_key = key
            .public_key()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let signature_hex = signature
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let event = verify_discord(
            &public_key,
            timestamp,
            &body(),
            &signature_hex,
            "interaction-1",
            "APPLICATION_COMMAND",
            1_700_000_000,
        )
        .expect("a verified interaction");
        assert_eq!(event.source, TriggerSource::Discord);

        let mut tampered = signature_hex.clone();
        tampered.replace_range(0..2, "00");
        assert!(matches!(
            verify_discord(
                &public_key,
                timestamp,
                &body(),
                &tampered,
                "interaction-1",
                "APPLICATION_COMMAND",
                1_700_000_000
            ),
            Err(TriggerError::InvalidSignature)
        ));
    }

    #[test]
    fn a_stale_discord_interaction_is_refused() {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public_key = key
            .public_key()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut message = b"1700000000".to_vec();
        message.extend_from_slice(&body());
        let signature_hex = key
            .sign(&message)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(matches!(
            verify_discord(
                &public_key,
                "1700000000",
                &body(),
                &signature_hex,
                "interaction-1",
                "APPLICATION_COMMAND",
                1_700_000_000 + SLACK_TOLERANCE_SECS + 1
            ),
            Err(TriggerError::StaleTimestamp)
        ));
    }

    #[test]
    fn a_scoped_bearer_token_is_checked() {
        assert!(verify_http_token("Bearer abc", "abc").is_ok());
        assert!(verify_http_token("bearer abc", "abc").is_ok());
        assert!(matches!(
            verify_http_token("Bearer abd", "abc"),
            Err(TriggerError::InvalidToken)
        ));
        assert!(matches!(
            verify_http_token("abc", "abc"),
            Err(TriggerError::InvalidToken)
        ));
    }
}
