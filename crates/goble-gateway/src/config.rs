use std::net::SocketAddr;
use std::time::Duration;

/// Default listen address. The gateway is loopback-only unless told otherwise.
pub const DEFAULT_BIND: &str = "127.0.0.1:8788";
/// How long a browser session stays valid after a successful PIN login.
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error(
        "refusing to bind {addr} without a PIN; use --pin-file or --pin \
         (loopback may use --no-auth explicitly)"
    )]
    NonLoopbackWithoutPin { addr: SocketAddr },
    #[error("the worker token must not be empty")]
    EmptyWorkerToken,
}

/// Configuration for the gateway.
///
/// `pin_hash` is the only form in which the client PIN is kept; the plaintext
/// PIN never enters the process after startup. `None` disables client auth and
/// is only valid on a loopback bind (the CLI reaches that state only through an
/// explicit `--no-auth`).
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bind: SocketAddr,
    pub pin_hash: Option<[u8; 32]>,
    pub worker_token: String,
    pub public_url: Option<String>,
    pub allowed_hosts: Vec<String>,
    pub session_ttl: Duration,
}

impl GatewayConfig {
    pub fn new(
        bind: SocketAddr,
        pin_hash: Option<[u8; 32]>,
        worker_token: impl Into<String>,
    ) -> Self {
        Self {
            bind,
            pin_hash,
            worker_token: worker_token.into(),
            public_url: None,
            allowed_hosts: Vec::new(),
            session_ttl: DEFAULT_SESSION_TTL,
        }
    }

    /// Reject configurations that would expose an unauthenticated listener.
    ///
    /// A non-loopback bind is always treated as public: it must have a PIN.
    /// Disabling auth is a loopback-only convenience for local development.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.bind.ip().is_loopback() && self.pin_hash.is_none() {
            return Err(ConfigError::NonLoopbackWithoutPin { addr: self.bind });
        }
        if self.worker_token.is_empty() {
            return Err(ConfigError::EmptyWorkerToken);
        }
        Ok(())
    }

    pub fn requires_auth(&self) -> bool {
        self.pin_hash.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::hash_pin;

    fn config_with(bind: &str, pin: Option<&str>) -> GatewayConfig {
        GatewayConfig::new(bind.parse().unwrap(), pin.map(hash_pin), "worker-token")
    }

    #[test]
    fn non_loopback_bind_without_a_pin_is_refused() {
        let config = config_with("0.0.0.0:8788", None);
        assert_eq!(
            config.validate(),
            Err(ConfigError::NonLoopbackWithoutPin {
                addr: "0.0.0.0:8788".parse().unwrap()
            })
        );
    }

    #[test]
    fn non_loopback_bind_with_a_pin_is_allowed() {
        assert!(config_with("0.0.0.0:8788", Some("123456"))
            .validate()
            .is_ok());
    }

    #[test]
    fn loopback_bind_without_a_pin_is_allowed_for_no_auth_mode() {
        assert!(config_with("127.0.0.1:8788", None).validate().is_ok());
    }
}
