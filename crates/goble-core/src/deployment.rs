use serde::{Deserialize, Serialize};

/// How the initial seed of a worker group is exposed to other peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum DeploymentMode {
    /// This desktop acts as the worker/seed. Reachability depends on LAN, UPnP, or port forwarding.
    Local {
        advertise_upnp: bool,
        local_port: u16,
    },
    /// A worker is deployed on a remote server with a public endpoint.
    RemoteServer {
        host: String,
        user: String,
        port: u16,
        private_key: String,
        endpoint: String,
    },
    /// The seed is reachable through a mesh VPN such as Tailscale or Headscale.
    MeshVpn {
        provider: MeshVpnProvider,
        auth_key: String,
        headscale_url: Option<String>,
        hostname: String,
    },
}

impl Default for DeploymentMode {
    fn default() -> Self {
        Self::Local {
            advertise_upnp: false,
            local_port: 8787,
        }
    }
}

impl DeploymentMode {
    pub fn mode_name(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::RemoteServer { .. } => "remote_server",
            Self::MeshVpn { .. } => "mesh_vpn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshVpnProvider {
    Tailscale,
    Headscale,
}

impl std::fmt::Display for MeshVpnProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tailscale => write!(f, "Tailscale"),
            Self::Headscale => write!(f, "Headscale"),
        }
    }
}

/// Persistable JSON configuration for a cluster membership.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentConfig {
    #[serde(flatten)]
    pub mode: DeploymentMode,
}

/// Reachability status returned to the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeploymentStatus {
    pub mode: String,
    pub local_endpoint: Option<String>,
    pub public_endpoint: Option<String>,
    pub mesh_hostname: Option<String>,
    pub upnp_mapped: Option<bool>,
    pub error: Option<String>,
}

impl DeploymentStatus {
    pub fn from_config(config: &DeploymentConfig, hostname: Option<&str>) -> Self {
        match &config.mode {
            DeploymentMode::Local {
                advertise_upnp,
                local_port,
            } => {
                let local = hostname
                    .map(|h| format!("{}:{}", h, local_port))
                    .or_else(|| Some(format!("127.0.0.1:{}", local_port)));
                Self {
                    mode: "local".to_string(),
                    local_endpoint: local,
                    public_endpoint: None,
                    mesh_hostname: None,
                    upnp_mapped: Some(*advertise_upnp),
                    error: None,
                }
            }
            DeploymentMode::RemoteServer { endpoint, .. } => Self {
                mode: "remote_server".to_string(),
                local_endpoint: None,
                public_endpoint: Some(endpoint.clone()),
                mesh_hostname: None,
                upnp_mapped: None,
                error: None,
            },
            DeploymentMode::MeshVpn { hostname, .. } => Self {
                mode: "mesh_vpn".to_string(),
                local_endpoint: None,
                public_endpoint: None,
                mesh_hostname: Some(hostname.clone()),
                upnp_mapped: None,
                error: None,
            },
        }
    }
}

/// Helper used to build a default deployment status for a membership without stored config.
pub fn default_deployment_status() -> DeploymentStatus {
    DeploymentStatus {
        mode: "local".to_string(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_mode_roundtrip() {
        let config = DeploymentConfig {
            mode: DeploymentMode::Local {
                advertise_upnp: true,
                local_port: 8787,
            },
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: DeploymentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.mode, restored.mode);
    }

    #[test]
    fn test_deployment_status_local() {
        let config = DeploymentConfig {
            mode: DeploymentMode::Local {
                advertise_upnp: false,
                local_port: 8787,
            },
        };
        let status = DeploymentStatus::from_config(&config, Some("desktop.local"));
        assert_eq!(status.mode, "local");
        assert_eq!(status.local_endpoint.as_deref(), Some("desktop.local:8787"));
    }
}
