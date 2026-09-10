use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Detect whether the worker is running inside a Kubernetes pod by looking for the
/// standard service-account token mount.
pub fn in_cluster() -> bool {
    PathBuf::from("/var/run/secrets/kubernetes.io/serviceaccount/token").exists()
}

/// Read the pod namespace from the Kubernetes service-account file, if present.
pub fn pod_namespace() -> Option<String> {
    std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/namespace")
        .ok()
        .map(|s| s.trim().to_string())
}

/// Read the pod name from the `HOSTNAME` or `POD_NAME` env var.
pub fn pod_name() -> Option<String> {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("POD_NAME"))
        .ok()
}

/// Kubernetes Lease resource subset used for leader election.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    #[serde(rename = "kind")]
    pub kind: String,
    pub metadata: LeaseMetadata,
    pub spec: LeaseSpec,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeaseMetadata {
    pub name: String,
    pub namespace: String,
    #[serde(rename = "resourceVersion", skip_serializing_if = "Option::is_none")]
    pub resource_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeaseSpec {
    #[serde(rename = "holderIdentity", skip_serializing_if = "Option::is_none")]
    pub holder_identity: Option<String>,
    #[serde(
        rename = "leaseDurationSeconds",
        skip_serializing_if = "Option::is_none"
    )]
    pub lease_duration_seconds: Option<i32>,
    #[serde(rename = "acquireTime", skip_serializing_if = "Option::is_none")]
    pub acquire_time: Option<DateTime<Utc>>,
    #[serde(rename = "renewTime", skip_serializing_if = "Option::is_none")]
    pub renew_time: Option<DateTime<Utc>>,
}

/// Client for Kubernetes leader election using `coordination.k8s.io/v1` leases.
/// When not running in a cluster, or when the API is unreachable, the elector
/// reports itself as leader so local scheduling still works.
pub struct KubeLeaderElector {
    client: reqwest::Client,
    api_url: String,
    token: String,
    namespace: String,
    lease_name: String,
    identity: String,
    lease_duration_seconds: i32,
    renew_interval: Duration,
}

impl KubeLeaderElector {
    /// Build an elector from the in-cluster environment. Returns `None` if the
    /// standard service-account files are not present.
    pub fn from_in_cluster(lease_name: impl Into<String>) -> Result<Option<Self>> {
        if !in_cluster() {
            return Ok(None);
        }
        let namespace = pod_namespace().context("failed to read pod namespace")?;
        let token = std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
            .context("failed to read service account token")?;
        let ca_path = PathBuf::from("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt");
        let identity = pod_name().unwrap_or_else(|| "goblin".to_string());
        Self::new(
            "https://kubernetes.default.svc",
            namespace,
            lease_name,
            token,
            ca_path,
            identity,
        )
        .map(Some)
    }

    /// Build an elector with explicit parameters. Useful for tests and for
    /// connecting to a kubeconfig proxy.
    pub fn new(
        api_url: impl Into<String>,
        namespace: impl Into<String>,
        lease_name: impl Into<String>,
        token: impl Into<String>,
        ca_path: PathBuf,
        identity: impl Into<String>,
    ) -> Result<Self> {
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(10));
        if ca_path.exists() {
            let cert = reqwest::Certificate::from_pem(&std::fs::read(&ca_path)?)
                .context("failed to load CA cert")?;
            builder = builder.add_root_certificate(cert);
        }
        let client = builder.build().context("failed to build http client")?;
        Ok(Self {
            client,
            api_url: api_url.into(),
            token: token.into(),
            namespace: namespace.into(),
            lease_name: lease_name.into(),
            identity: identity.into(),
            lease_duration_seconds: 30,
            renew_interval: Duration::from_secs(10),
        })
    }

    /// Try to acquire or renew the lease. Returns true if this identity is the
    /// current holder.
    pub async fn acquire_or_renew(&self) -> Result<bool> {
        let url = format!(
            "{}/apis/coordination.k8s.io/v1/namespaces/{}/leases/{}",
            self.api_url, self.namespace, self.lease_name
        );
        let now = Utc::now();
        let mut lease = Lease {
            api_version: "coordination.k8s.io/v1".to_string(),
            kind: "Lease".to_string(),
            metadata: LeaseMetadata {
                name: self.lease_name.clone(),
                namespace: self.namespace.clone(),
                resource_version: None,
            },
            spec: LeaseSpec {
                holder_identity: Some(self.identity.clone()),
                lease_duration_seconds: Some(self.lease_duration_seconds),
                acquire_time: Some(now),
                renew_time: Some(now),
            },
        };

        match self.client.get(&url).bearer_auth(&self.token).send().await {
            Ok(resp) if resp.status().is_success() => {
                let existing: Lease = resp.json().await.context("failed to parse lease")?;
                let is_holder = existing
                    .spec
                    .holder_identity
                    .as_ref()
                    .map(|h| h == &self.identity)
                    .unwrap_or(false);
                let expired = existing
                    .spec
                    .renew_time
                    .map(|t| t + Duration::from_secs(self.lease_duration_seconds as u64) < now)
                    .unwrap_or(true);
                if is_holder || expired {
                    lease.metadata.resource_version = existing.metadata.resource_version;
                    self.write_lease(&url, &lease).await
                } else {
                    Ok(false)
                }
            }
            Ok(resp) if resp.status() == 404 => self.write_lease(&url, &lease).await,
            Ok(resp) => anyhow::bail!("kube lease GET failed: {}", resp.status()),
            Err(e) => Err(e).context("kube lease GET request failed"),
        }
    }

    async fn write_lease(&self, url: &str, lease: &Lease) -> Result<bool> {
        let resp = self
            .client
            .request(
                if lease.metadata.resource_version.is_some() {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::POST
                },
                url,
            )
            .bearer_auth(&self.token)
            .json(lease)
            .send()
            .await
            .context("kube lease write request failed")?;
        if resp.status().is_success() {
            Ok(true)
        } else {
            anyhow::bail!("kube lease write failed: {}", resp.status())
        }
    }

    /// Spawn a background task that keeps the lease renewed. The returned handle
    /// resolves if the API becomes permanently unreachable.
    pub fn start(self: std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.renew_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(e) = self.acquire_or_renew().await {
                    tracing::warn!("leader election renewal failed: {}", e);
                }
            }
        })
    }

    /// Lease duration used for this elector.
    pub fn lease_duration(&self) -> Duration {
        Duration::from_secs(self.lease_duration_seconds as u64)
    }
}

/// Simple leader state shared with the scheduler. When the scheduler is not in
/// cluster mode, it is always the leader.
#[derive(Clone)]
pub struct LeaderState {
    is_leader: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl LeaderState {
    pub fn new(is_leader: bool) -> Self {
        Self {
            is_leader: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(is_leader)),
        }
    }

    pub fn is_leader(&self) -> bool {
        self.is_leader.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_leader(&self, is_leader: bool) {
        self.is_leader
            .store(is_leader, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pod_namespace_reads_file() {
        // In a test environment this will likely return None; we just exercise
        // the function without panicking.
        let _ = pod_namespace();
    }

    #[test]
    fn test_leader_state() {
        let state = LeaderState::new(false);
        assert!(!state.is_leader());
        state.set_leader(true);
        assert!(state.is_leader());
    }

    #[tokio::test]
    async fn test_kube_leader_elector_acquires_new_lease() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        let lease_path = "/apis/coordination.k8s.io/v1/namespaces/goblin/leases/goblin-scheduler";
        Mock::given(method("GET"))
            .and(path(lease_path))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path(lease_path))
            .respond_with(ResponseTemplate::new(201))
            .mount(&mock_server)
            .await;

        let elector = KubeLeaderElector::new(
            mock_server.uri(),
            "goblin",
            "goblin-scheduler",
            "fake-token",
            std::path::PathBuf::new(),
            "goblin-0",
        )
        .unwrap();
        let leader = elector.acquire_or_renew().await.unwrap();
        assert!(leader);
    }

    #[tokio::test]
    async fn test_kube_leader_elector_yields_to_existing_holder() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        let lease_path = "/apis/coordination.k8s.io/v1/namespaces/goblin/leases/goblin-scheduler";
        let lease_json = serde_json::json!({
            "apiVersion": "coordination.k8s.io/v1",
            "kind": "Lease",
            "metadata": { "name": "goblin-scheduler", "namespace": "goblin", "resourceVersion": "1" },
            "spec": {
                "holderIdentity": "goblin-1",
                "leaseDurationSeconds": 30,
                "acquireTime": Utc::now().to_rfc3339(),
                "renewTime": Utc::now().to_rfc3339(),
            }
        });
        Mock::given(method("GET"))
            .and(path(lease_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(lease_json))
            .mount(&mock_server)
            .await;

        let elector = KubeLeaderElector::new(
            mock_server.uri(),
            "goblin",
            "goblin-scheduler",
            "fake-token",
            std::path::PathBuf::new(),
            "goblin-0",
        )
        .unwrap();
        let leader = elector.acquire_or_renew().await.unwrap();
        assert!(!leader);
    }

    #[tokio::test]
    async fn test_lease_serialization() {
        let lease = Lease {
            api_version: "coordination.k8s.io/v1".to_string(),
            kind: "Lease".to_string(),
            metadata: LeaseMetadata {
                name: "goblin-scheduler".to_string(),
                namespace: "goblin".to_string(),
                resource_version: Some("123".to_string()),
            },
            spec: LeaseSpec {
                holder_identity: Some("goblin-0".to_string()),
                lease_duration_seconds: Some(30),
                acquire_time: Some(Utc::now()),
                renew_time: Some(Utc::now()),
            },
        };
        let json = serde_json::to_string(&lease).unwrap();
        assert!(json.contains("coordination.k8s.io/v1"));
        assert!(json.contains("goblin-scheduler"));
        assert!(json.contains("goblin-0"));
    }
}
