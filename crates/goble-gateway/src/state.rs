use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::extract::ws::Message;
use axum::http::{header, HeaderMap};
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tokio::sync::{mpsc, watch};

use crate::assets;
use crate::auth;
use crate::config::{ConfigError, GatewayConfig};

/// Name of the browser session cookie.
pub const SESSION_COOKIE: &str = "goble_gateway_session";

const WORKER_OUTBOUND_CAPACITY: usize = 256;
const CLIENT_OUTBOUND_CAPACITY: usize = 128;
const LOGIN_DELAY_THRESHOLD: u32 = 3;
const LOGIN_DELAY_STEP: Duration = Duration::from_millis(250);
const LOGIN_DELAY_CAP: u32 = 8;
const MAX_TRACKED_FAILURES: u32 = 1_000;

struct Session {
    expires: Instant,
}

pub(crate) struct WorkerEntry {
    id: String,
    connection_id: String,
    connected_at: DateTime<Utc>,
    last_seen: DateTime<Utc>,
    outbound: mpsc::Sender<Message>,
    shutdown: watch::Sender<bool>,
    clients: HashMap<u64, ClientHandle>,
}

struct ClientHandle {
    sender: mpsc::Sender<Message>,
    shutdown: watch::Sender<bool>,
}

struct LoginLimiter {
    failures: u32,
}

/// A snapshot of one connected worker, as shown on `/panel` and returned by
/// `/api/workers`. It deliberately carries no token or connection id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerSummary {
    pub id: String,
    pub connected_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

/// Shared, in-memory gateway state.
///
/// Everything here is process-local: sessions and worker registrations do not
/// survive a restart. The gateway is a transport and access-control point, not
/// a persistence layer.
pub struct GatewayState {
    config: GatewayConfig,
    allowed_hosts: Vec<String>,
    web_root: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
    workers: RwLock<HashMap<String, WorkerEntry>>,
    login: Mutex<LoginLimiter>,
    next_client_id: AtomicU64,
}

impl GatewayState {
    pub fn new(config: GatewayConfig) -> Result<Self, ConfigError> {
        config.validate()?;

        let mut allowed_hosts: Vec<String> = config
            .allowed_hosts
            .iter()
            .filter_map(|host| normalize_host(host))
            .collect();
        if let Some(public_url) = &config.public_url {
            if let Some(host) = normalize_host(public_url) {
                allowed_hosts.push(host);
            }
        }
        if !config.bind.ip().is_unspecified() {
            if let Some(host) = normalize_host(&config.bind.ip().to_string()) {
                allowed_hosts.push(host);
            }
        }
        allowed_hosts.sort();
        allowed_hosts.dedup();

        let web_root = assets::resolve_web_dir(config.web_dir.as_deref());

        Ok(Self {
            config,
            allowed_hosts,
            web_root,
            sessions: Mutex::new(HashMap::new()),
            workers: RwLock::new(HashMap::new()),
            login: Mutex::new(LoginLimiter { failures: 0 }),
            next_client_id: AtomicU64::new(1),
        })
    }

    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    /// Directory the browser client is served from. It may not exist yet; the
    /// asset handler reports that as a build page rather than an error.
    pub fn web_root(&self) -> &Path {
        &self.web_root
    }

    pub fn requires_auth(&self) -> bool {
        self.config.requires_auth()
    }

    pub fn worker_count(&self) -> usize {
        self.workers.read().len()
    }

    pub fn workers(&self) -> Vec<WorkerSummary> {
        let mut workers: Vec<WorkerSummary> = self
            .workers
            .read()
            .values()
            .map(|entry| WorkerSummary {
                id: entry.id.clone(),
                connected_at: entry.connected_at,
                last_seen: entry.last_seen,
            })
            .collect();
        workers.sort_by(|left, right| left.id.cmp(&right.id));
        workers
    }

    pub(crate) fn has_worker(&self, id: &str) -> bool {
        self.workers.read().contains_key(id)
    }

    pub(crate) fn worker_current(&self, id: &str, connection_id: &str) -> bool {
        self.workers
            .read()
            .get(id)
            .map(|entry| entry.connection_id == connection_id)
            .unwrap_or(false)
    }

    /// Register a worker connection, replacing any previous connection under
    /// the same id. Existing clients are moved to the new connection.
    pub(crate) fn register_worker(
        &self,
        id: String,
    ) -> (String, mpsc::Receiver<Message>, watch::Receiver<bool>) {
        let (outbound, outbound_rx) = mpsc::channel(WORKER_OUTBOUND_CAPACITY);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let connection_id = auth::generate_token();

        let mut workers = self.workers.write();
        let clients = match workers.remove(&id) {
            Some(previous) => {
                let _ = previous.shutdown.send(true);
                previous.clients
            }
            None => HashMap::new(),
        };
        let now = Utc::now();
        workers.insert(
            id.clone(),
            WorkerEntry {
                id,
                connection_id: connection_id.clone(),
                connected_at: now,
                last_seen: now,
                outbound,
                shutdown,
                clients,
            },
        );

        (connection_id, outbound_rx, shutdown_rx)
    }

    /// Mark a worker disconnected, but only if this is still its current
    /// connection (an older socket must not evict its replacement).
    pub(crate) fn disconnect_worker(&self, id: &str, connection_id: &str) {
        let mut workers = self.workers.write();
        let is_current = workers
            .get(id)
            .map(|entry| entry.connection_id == connection_id)
            .unwrap_or(false);
        if !is_current {
            return;
        }
        if let Some(entry) = workers.remove(id) {
            for client in entry.clients.into_values() {
                let _ = client.shutdown.send(true);
            }
        }
    }

    /// Relay an opaque worker frame to every client attached to that worker.
    /// Returns `false` when the connection has been superseded.
    pub(crate) fn relay_from_worker(
        &self,
        id: &str,
        connection_id: &str,
        message: Message,
    ) -> bool {
        let mut workers = self.workers.write();
        let Some(entry) = workers.get_mut(id) else {
            return false;
        };
        if entry.connection_id != connection_id {
            return false;
        }
        entry.last_seen = Utc::now();
        entry.clients.retain(|_, client| {
            if client.sender.try_send(message.clone()).is_ok() {
                true
            } else {
                let _ = client.shutdown.send(true);
                false
            }
        });
        true
    }

    pub(crate) fn register_client(
        &self,
        worker_id: &str,
    ) -> Option<(u64, mpsc::Receiver<Message>, watch::Receiver<bool>)> {
        let mut workers = self.workers.write();
        let entry = workers.get_mut(worker_id)?;
        let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel(CLIENT_OUTBOUND_CAPACITY);
        let (shutdown, shutdown_rx) = watch::channel(false);
        entry
            .clients
            .insert(client_id, ClientHandle { sender, shutdown });
        Some((client_id, receiver, shutdown_rx))
    }

    pub(crate) fn unregister_client(&self, worker_id: &str, client_id: u64) {
        let mut workers = self.workers.write();
        if let Some(entry) = workers.get_mut(worker_id) {
            if let Some(client) = entry.clients.remove(&client_id) {
                let _ = client.shutdown.send(true);
            }
        }
    }

    /// Send a client frame to the worker's current connection. Looking up the
    /// sender each time keeps clients attached across a worker reconnect.
    pub(crate) async fn send_to_worker(&self, worker_id: &str, message: Message) -> bool {
        let sender = {
            let workers = self.workers.read();
            workers.get(worker_id).map(|entry| entry.outbound.clone())
        };
        match sender {
            Some(sender) => sender.send(message).await.is_ok(),
            None => false,
        }
    }

    pub(crate) fn worker_authorized(&self, headers: &HeaderMap) -> bool {
        let token = bearer_token(headers)
            .or_else(|| header_string(headers, "x-worker-token"))
            .unwrap_or_default();
        auth::constant_time_eq(self.config.worker_token.as_bytes(), token.as_bytes())
    }

    pub(crate) fn client_authorized(&self, headers: &HeaderMap) -> bool {
        if !self.requires_auth() {
            return true;
        }
        self.session_token(headers)
            .map(|token| self.session_valid(&token))
            .unwrap_or(false)
    }

    pub(crate) fn session_token(&self, headers: &HeaderMap) -> Option<String> {
        cookie_value(headers, SESSION_COOKIE)
    }

    pub(crate) fn create_session(&self) -> String {
        let token = auth::generate_token();
        let now = Instant::now();
        let expires = now + self.config.session_ttl;
        let mut sessions = self.sessions.lock();
        sessions.retain(|_, session| session.expires > now);
        sessions.insert(token.clone(), Session { expires });
        token
    }

    pub(crate) fn session_valid(&self, token: &str) -> bool {
        let now = Instant::now();
        let mut sessions = self.sessions.lock();
        sessions.retain(|_, session| session.expires > now);
        sessions
            .get(token)
            .map(|session| session.expires > now)
            .unwrap_or(false)
    }

    pub(crate) fn destroy_session(&self, token: &str) {
        self.sessions.lock().remove(token);
    }

    pub(crate) fn login_delay(&self) -> Duration {
        let failures = self.login.lock().failures;
        if failures < LOGIN_DELAY_THRESHOLD {
            Duration::ZERO
        } else {
            let steps = (failures - LOGIN_DELAY_THRESHOLD + 1).min(LOGIN_DELAY_CAP);
            LOGIN_DELAY_STEP * steps
        }
    }

    pub(crate) fn record_login_failure(&self) {
        let mut limiter = self.login.lock();
        limiter.failures = (limiter.failures + 1).min(MAX_TRACKED_FAILURES);
    }

    pub(crate) fn record_login_success(&self) {
        self.login.lock().failures = 0;
    }

    /// Host-header allowlist. Loopback names/IPs are always accepted; other
    /// hosts must be the bind address, the public URL host, or `--allow-host`.
    pub(crate) fn host_allowed(&self, raw_host: &str) -> bool {
        let Some(host) = normalize_host(raw_host) else {
            return false;
        };
        if host == "localhost" {
            return true;
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            if ip.is_loopback() {
                return true;
            }
        }
        self.allowed_hosts.iter().any(|allowed| allowed == &host)
    }
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}

/// Reduce a Host header or configured URL to a bare lowercase hostname.
fn normalize_host(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let without_scheme = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);

    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next()?
    } else if authority.matches(':').count() == 1 {
        authority.split(':').next()?
    } else {
        authority
    };

    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_normalization_strips_scheme_port_and_brackets() {
        assert_eq!(
            normalize_host("example.com:8788").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_host("https://Example.COM/path").as_deref(),
            Some("example.com")
        );
        assert_eq!(normalize_host("[::1]:8788").as_deref(), Some("::1"));
        assert_eq!(normalize_host("localhost.").as_deref(), Some("localhost"));
        assert_eq!(normalize_host("  ").as_deref(), None);
    }

    #[test]
    fn unknown_hosts_are_rejected_but_loopback_is_accepted() {
        let state = GatewayState::new(GatewayConfig::new(
            "127.0.0.1:8788".parse().unwrap(),
            Some(auth::hash_pin("123456")),
            "worker-token",
        ))
        .unwrap();

        assert!(state.host_allowed("127.0.0.1:8788"));
        assert!(state.host_allowed("localhost:8788"));
        assert!(state.host_allowed("[::1]:8788"));
        assert!(!state.host_allowed("attacker.example:8788"));
    }

    #[test]
    fn configured_hosts_and_public_url_are_accepted() {
        let mut config = GatewayConfig::new(
            "0.0.0.0:8788".parse().unwrap(),
            Some(auth::hash_pin("123456")),
            "worker-token",
        );
        config.public_url = Some("https://gw.example.com".to_string());
        config.allowed_hosts = vec!["extra.example:8788".to_string()];
        let state = GatewayState::new(config).unwrap();

        assert!(state.host_allowed("gw.example.com"));
        assert!(state.host_allowed("extra.example:8788"));
        assert!(!state.host_allowed("other.example"));
    }
}
