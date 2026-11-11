//! Relay account sessions.
//!
//! A passkey proves who is at the browser; these tokens keep that proof alive
//! without asking for a biometric on every request. Access tokens are short
//! lived, refresh tokens rotate on every use and are bound to the device
//! certificate's fingerprint. A sensitive action needs a fresh assertion, not
//! just a valid session.
//!
//! Only hashes of the tokens are kept in memory. A stolen relay memory dump
//! therefore does not hand out usable bearer tokens.

use std::collections::HashMap;

use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};

/// A browser session may make ordinary calls for fifteen minutes.
pub const ACCESS_TTL_SECS: i64 = 15 * 60;
/// A device may refresh for thirty days, and each refresh rotates the token.
pub const REFRESH_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// An approval is good for five minutes after the user verification.
pub const STEP_UP_TTL_SECS: i64 = 5 * 60;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("the access token is not valid")]
    InvalidAccess,
    #[error("the access token expired")]
    AccessExpired,
    #[error("the refresh token is not valid")]
    InvalidRefresh,
    #[error("the refresh token expired")]
    RefreshExpired,
    #[error("the refresh token belongs to another device key")]
    BindingMismatch,
    #[error("a fresh user verification is required")]
    StepUpRequired,
    #[error("the system random source failed")]
    Random,
}

/// The pair a browser stores. The refresh token is set as an `HttpOnly`,
/// `Secure`, `SameSite=Strict` cookie; the access token is held in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: i64,
    pub refresh_expires_at: i64,
}

/// What a valid access token proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Authenticated {
    pub step_up_fresh: bool,
}

struct SessionRecord {
    device_id: String,
    /// SHA-256 of the device certificate's public key. A browser has no
    /// Ed25519 keystore key, so for a browser this is the passkey credential's
    /// public key fingerprint.
    certificate_fingerprint: [u8; 32],
    access_expires_at: i64,
    refresh_expires_at: i64,
    last_step_up: Option<i64>,
}

/// The relay's in-memory session table. A durable relay stores the same rows
/// in its registry; the token format and checks stay identical.
#[derive(Default)]
pub struct AuthSessions {
    by_access: HashMap<[u8; 32], SessionRecord>,
    by_refresh: HashMap<[u8; 32], [u8; 32]>,
}

impl AuthSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a session after a successful passkey assertion.
    pub fn issue(
        &mut self,
        device_id: impl Into<String>,
        certificate_fingerprint: [u8; 32],
        now: i64,
    ) -> Result<IssuedTokens, AuthError> {
        self.issue_at(device_id.into(), certificate_fingerprint, now, None)
    }

    fn issue_at(
        &mut self,
        device_id: String,
        certificate_fingerprint: [u8; 32],
        now: i64,
        last_step_up: Option<i64>,
    ) -> Result<IssuedTokens, AuthError> {
        let access_token = token()?;
        let refresh_token = token()?;
        let access_hash = hash_token(&access_token);
        let refresh_hash = hash_token(&refresh_token);
        let record = SessionRecord {
            device_id,
            certificate_fingerprint,
            access_expires_at: now + ACCESS_TTL_SECS,
            refresh_expires_at: now + REFRESH_TTL_SECS,
            last_step_up,
        };
        self.by_access.insert(access_hash, record);
        self.by_refresh.insert(refresh_hash, access_hash);
        Ok(IssuedTokens {
            access_token,
            refresh_token,
            access_expires_at: now + ACCESS_TTL_SECS,
            refresh_expires_at: now + REFRESH_TTL_SECS,
        })
    }

    /// Check an access token. The device id is not returned to the caller
    /// because the token already names one device and the relay's routing uses
    /// the certificate, not this table.
    pub fn authenticate(&self, access_token: &str, now: i64) -> Result<Authenticated, AuthError> {
        let record = self
            .by_access
            .get(&hash_token(access_token))
            .ok_or(AuthError::InvalidAccess)?;
        if now >= record.access_expires_at {
            return Err(AuthError::AccessExpired);
        }
        Ok(Authenticated {
            step_up_fresh: is_fresh(record.last_step_up, now),
        })
    }

    /// Rotate the refresh token. The new token is bound to the same device
    /// certificate; using it with a different key is refused and leaves the
    /// original session alive.
    pub fn refresh(
        &mut self,
        refresh_token: &str,
        certificate_fingerprint: [u8; 32],
        now: i64,
    ) -> Result<IssuedTokens, AuthError> {
        let refresh_hash = hash_token(refresh_token);
        let access_hash = *self
            .by_refresh
            .get(&refresh_hash)
            .ok_or(AuthError::InvalidRefresh)?;
        let record = self
            .by_access
            .get(&access_hash)
            .ok_or(AuthError::InvalidRefresh)?;
        if now >= record.refresh_expires_at {
            self.by_access.remove(&access_hash);
            self.by_refresh.remove(&refresh_hash);
            return Err(AuthError::RefreshExpired);
        }
        if record.certificate_fingerprint != certificate_fingerprint {
            return Err(AuthError::BindingMismatch);
        }
        let device_id = record.device_id.clone();
        let last_step_up = record.last_step_up;
        self.by_access.remove(&access_hash);
        self.by_refresh.remove(&refresh_hash);
        self.issue_at(device_id, certificate_fingerprint, now, last_step_up)
    }

    /// Record a fresh user verification for this session.
    pub fn step_up(&mut self, access_token: &str, now: i64) -> Result<(), AuthError> {
        let record = self
            .by_access
            .get_mut(&hash_token(access_token))
            .ok_or(AuthError::InvalidAccess)?;
        if now >= record.access_expires_at {
            return Err(AuthError::AccessExpired);
        }
        record.last_step_up = Some(now);
        Ok(())
    }

    /// Gate a sensitive action: approve a run, open a write grant, add or
    /// revoke a device.
    pub fn require_step_up(&self, access_token: &str, now: i64) -> Result<(), AuthError> {
        let record = self
            .by_access
            .get(&hash_token(access_token))
            .ok_or(AuthError::InvalidAccess)?;
        if now >= record.access_expires_at {
            return Err(AuthError::AccessExpired);
        }
        if !is_fresh(record.last_step_up, now) {
            return Err(AuthError::StepUpRequired);
        }
        Ok(())
    }

    /// End one browser session.
    pub fn logout(&mut self, access_token: &str) -> bool {
        let access_hash = hash_token(access_token);
        let removed = self.by_access.remove(&access_hash).is_some();
        self.by_refresh.retain(|_, target| *target != access_hash);
        removed
    }

    /// Revoke every session for a device, used when the device certificate is
    /// revoked.
    pub fn revoke_device(&mut self, device_id: &str) -> usize {
        let removed: Vec<[u8; 32]> = self
            .by_access
            .iter()
            .filter(|(_, record)| record.device_id == device_id)
            .map(|(hash, _)| *hash)
            .collect();
        for access_hash in &removed {
            self.by_access.remove(access_hash);
            self.by_refresh.retain(|_, target| target != access_hash);
        }
        removed.len()
    }

    /// Drop sessions whose refresh window has ended.
    pub fn purge(&mut self, now: i64) -> usize {
        let expired: Vec<[u8; 32]> = self
            .by_access
            .iter()
            .filter(|(_, record)| now >= record.refresh_expires_at)
            .map(|(hash, _)| *hash)
            .collect();
        for access_hash in &expired {
            self.by_access.remove(access_hash);
            self.by_refresh.retain(|_, target| target != access_hash);
        }
        expired.len()
    }

    pub fn len(&self) -> usize {
        self.by_access.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_access.is_empty()
    }
}

fn is_fresh(last_step_up: Option<i64>, now: i64) -> bool {
    last_step_up
        .map(|at| now >= at && now - at <= STEP_UP_TTL_SECS)
        .unwrap_or(false)
}

fn token() -> Result<String, AuthError> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| AuthError::Random)?;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;
    const KEY: [u8; 32] = [1u8; 32];
    const OTHER_KEY: [u8; 32] = [2u8; 32];

    fn session() -> (AuthSessions, IssuedTokens) {
        let mut sessions = AuthSessions::new();
        let tokens = sessions.issue("phone", KEY, NOW).unwrap();
        (sessions, tokens)
    }

    #[test]
    fn a_fresh_access_token_authenticates() {
        let (sessions, tokens) = session();
        let authenticated = sessions
            .authenticate(&tokens.access_token, NOW + 1)
            .unwrap();
        assert!(!authenticated.step_up_fresh);
        assert_eq!(tokens.access_expires_at, NOW + ACCESS_TTL_SECS);
        assert_eq!(tokens.refresh_expires_at, NOW + REFRESH_TTL_SECS);
    }

    #[test]
    fn an_unknown_access_token_is_refused() {
        let (sessions, _tokens) = session();
        assert!(matches!(
            sessions.authenticate("not-a-token", NOW),
            Err(AuthError::InvalidAccess)
        ));
    }

    #[test]
    fn an_expired_access_token_is_refused() {
        let (sessions, tokens) = session();
        assert!(matches!(
            sessions.authenticate(&tokens.access_token, NOW + ACCESS_TTL_SECS),
            Err(AuthError::AccessExpired)
        ));
    }

    #[test]
    fn two_sessions_get_different_tokens() {
        let mut sessions = AuthSessions::new();
        let first = sessions.issue("phone", KEY, NOW).unwrap();
        let second = sessions.issue("phone", KEY, NOW).unwrap();
        assert_ne!(first.access_token, second.access_token);
        assert_ne!(first.refresh_token, second.refresh_token);
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn refresh_rotates_both_tokens() {
        let (mut sessions, tokens) = session();
        let next = sessions
            .refresh(&tokens.refresh_token, KEY, NOW + 60)
            .unwrap();
        assert_ne!(next.access_token, tokens.access_token);
        assert_ne!(next.refresh_token, tokens.refresh_token);
        assert!(matches!(
            sessions.authenticate(&tokens.access_token, NOW + 61),
            Err(AuthError::InvalidAccess)
        ));
        assert!(sessions.authenticate(&next.access_token, NOW + 61).is_ok());
        assert!(matches!(
            sessions.refresh(&tokens.refresh_token, KEY, NOW + 61),
            Err(AuthError::InvalidRefresh)
        ));
    }

    #[test]
    fn a_refresh_works_after_the_access_token_expired() {
        let (mut sessions, tokens) = session();
        let later = NOW + ACCESS_TTL_SECS + 1;
        let next = sessions.refresh(&tokens.refresh_token, KEY, later).unwrap();
        assert!(sessions.authenticate(&next.access_token, later).is_ok());
    }

    #[test]
    fn a_refresh_bound_to_another_key_is_refused() {
        let (mut sessions, tokens) = session();
        assert!(matches!(
            sessions.refresh(&tokens.refresh_token, OTHER_KEY, NOW + 1),
            Err(AuthError::BindingMismatch)
        ));
        // The original session still works after the failed attempt.
        assert!(sessions
            .refresh(&tokens.refresh_token, KEY, NOW + 1)
            .is_ok());
    }

    #[test]
    fn an_expired_refresh_is_refused() {
        let (mut sessions, tokens) = session();
        assert!(matches!(
            sessions.refresh(&tokens.refresh_token, KEY, NOW + REFRESH_TTL_SECS),
            Err(AuthError::RefreshExpired)
        ));
        assert!(sessions.is_empty());
    }

    #[test]
    fn step_up_is_fresh_for_five_minutes() {
        let (mut sessions, tokens) = session();
        assert!(matches!(
            sessions.require_step_up(&tokens.access_token, NOW + 1),
            Err(AuthError::StepUpRequired)
        ));
        sessions.step_up(&tokens.access_token, NOW + 10).unwrap();
        assert!(sessions
            .require_step_up(&tokens.access_token, NOW + STEP_UP_TTL_SECS)
            .is_ok());
        assert!(matches!(
            sessions.require_step_up(&tokens.access_token, NOW + STEP_UP_TTL_SECS + 11),
            Err(AuthError::StepUpRequired)
        ));
    }

    #[test]
    fn step_up_survives_a_refresh() {
        let (mut sessions, tokens) = session();
        sessions.step_up(&tokens.access_token, NOW).unwrap();
        let next = sessions
            .refresh(&tokens.refresh_token, KEY, NOW + 1)
            .unwrap();
        assert!(sessions
            .require_step_up(&next.access_token, NOW + 2)
            .is_ok());
    }

    #[test]
    fn logout_ends_only_one_session() {
        let mut sessions = AuthSessions::new();
        let first = sessions.issue("phone", KEY, NOW).unwrap();
        let second = sessions.issue("tablet", KEY, NOW).unwrap();
        assert!(sessions.logout(&first.access_token));
        assert!(matches!(
            sessions.authenticate(&first.access_token, NOW + 1),
            Err(AuthError::InvalidAccess)
        ));
        assert!(sessions.authenticate(&second.access_token, NOW + 1).is_ok());
        assert!(!sessions.logout("unknown"));
    }

    #[test]
    fn revoking_a_device_ends_all_of_its_sessions() {
        let mut sessions = AuthSessions::new();
        let phone_one = sessions.issue("phone", KEY, NOW).unwrap();
        let phone_two = sessions.issue("phone", KEY, NOW).unwrap();
        let tablet = sessions.issue("tablet", KEY, NOW).unwrap();
        assert_eq!(sessions.revoke_device("phone"), 2);
        assert!(matches!(
            sessions.refresh(&phone_one.refresh_token, KEY, NOW + 1),
            Err(AuthError::InvalidRefresh)
        ));
        assert!(matches!(
            sessions.refresh(&phone_two.refresh_token, KEY, NOW + 1),
            Err(AuthError::InvalidRefresh)
        ));
        assert!(sessions
            .refresh(&tablet.refresh_token, KEY, NOW + 1)
            .is_ok());
    }

    #[test]
    fn purge_removes_only_expired_sessions() {
        let mut sessions = AuthSessions::new();
        let old = sessions.issue("phone", KEY, NOW).unwrap();
        let fresh = sessions
            .issue("tablet", KEY, NOW + REFRESH_TTL_SECS)
            .unwrap();
        assert_eq!(sessions.purge(NOW + REFRESH_TTL_SECS), 1);
        assert!(matches!(
            sessions.refresh(&old.refresh_token, KEY, NOW + REFRESH_TTL_SECS),
            Err(AuthError::InvalidRefresh)
        ));
        assert!(sessions
            .refresh(&fresh.refresh_token, KEY, NOW + REFRESH_TTL_SECS + 1)
            .is_ok());
    }
}
