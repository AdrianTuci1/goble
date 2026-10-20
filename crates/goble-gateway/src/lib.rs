//! Always-on WebSocket session gateway for Goble workers.
//!
//! The gateway is a relay and access-control point, not a terminal renderer:
//!
//! - Workers connect to `/ws/worker` with a shared worker bearer token.
//! - Browsers unlock `/` with a PIN, receive an HttpOnly session cookie, and
//!   open `/ws/client?worker=<id>` from `/panel`.
//! - Relayed text and binary frames are copied verbatim in both directions.
//!
//! It can run as a standalone server (VPS, Tailscale node) or be spawned by the
//! desktop application; nothing here depends on the GUI. Listener TLS is
//! deliberately out of scope: terminate it in Tailscale or a reverse proxy.
//!
//! State is in memory only. Sessions and worker registrations are lost on
//! restart, which is appropriate for a transport: workers reconnect and users
//! enter the PIN again.

mod auth;
mod config;
mod http;
mod state;
mod web;
mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;

pub use auth::{constant_time_eq, generate_pin, generate_token, hash_pin, verify_pin};
pub use config::{ConfigError, GatewayConfig, DEFAULT_BIND, DEFAULT_SESSION_TTL};
pub use state::{GatewayState, WorkerSummary, SESSION_COOKIE};

/// A configured gateway. Construct it, then serve [`Gateway::router`].
pub struct Gateway {
    state: Arc<GatewayState>,
}

impl Gateway {
    /// Validate the configuration and build the in-memory gateway.
    pub fn new(config: GatewayConfig) -> Result<Self, ConfigError> {
        Ok(Self {
            state: Arc::new(GatewayState::new(config)?),
        })
    }

    pub fn state(&self) -> Arc<GatewayState> {
        self.state.clone()
    }

    pub fn router(&self) -> Router {
        http::router(self.state.clone())
    }

    pub fn bind_addr(&self) -> SocketAddr {
        self.state.config().bind
    }
}

/// Build the HTTP router around an existing state (used by tests and embedders).
pub fn router(state: Arc<GatewayState>) -> Router {
    http::router(state)
}
