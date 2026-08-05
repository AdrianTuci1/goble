use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::identity::{ClusterCa, ClusterRole, Identity};

/// Stored invite record. The PEM bundle is the canonical credential; the code is a
/// short handle that maps to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInvite {
    pub id: String,
    pub cluster_name: String,
    pub code: String,
    /// PEM bundle containing the device certificate, device key, and CA certificate.
    pub pem_bundle: String,
    pub revoked: bool,
    pub created_at: String,
}

impl ClusterInvite {
    /// Generate a random 12-character invite code.
    pub fn generate_code() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
        let mut rng = rand::rng();
        (0..12)
            .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
            .collect()
    }

    /// Issue a new invite for a cluster using its CA. The invite grants the given role.
    pub fn generate(
        cluster_name: &str,
        ca: &ClusterCa,
        role: ClusterRole,
    ) -> Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        let code = Self::generate_code();
        let device_serial = format!("invite-{}", &id[..8]);
        let device = ca.sign_device(&device_serial, role, 365)?;
        let pem_bundle = format!(
            "{}\n{}\n{}",
            device.cert_pem.trim(),
            device.key_pem.trim(),
            ca.identity.cert_pem.trim()
        );
        Ok(Self {
            id,
            cluster_name: cluster_name.to_string(),
            code,
            pem_bundle,
            revoked: false,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Parse the PEM bundle into a usable identity. This validates that the
    /// certificate is signed by the bundled CA and extracts the role.
    pub fn to_identity(&self) -> Result<Identity> {
        Identity::from_pem(
            Self::extract_pem(&self.pem_bundle, "CERTIFICATE")
                .context("missing certificate in invite")?,
            Self::extract_pem(&self.pem_bundle, "PRIVATE KEY")
                .context("missing private key in invite")?,
        )
    }

    fn extract_pem(bundle: &str, label: &str) -> Option<String> {
        let start = format!("-----BEGIN {}-----", label);
        let end = format!("-----END {}-----", label);
        let start_idx = bundle.find(&start)?;
        let end_idx = bundle.find(&end)? + end.len();
        Some(bundle[start_idx..end_idx].to_string())
    }
}

/// Serializable payload used for invite transport, including a human-friendly code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInvitePayload {
    pub cluster_name: String,
    pub code: String,
    pub pem_bundle: String,
}

impl ClusterInvitePayload {
    pub fn from_invite(invite: &ClusterInvite) -> Self {
        Self {
            cluster_name: invite.cluster_name.clone(),
            code: invite.code.clone(),
            pem_bundle: invite.pem_bundle.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_invite() {
        let ca = ClusterCa::generate_new("test-cluster").unwrap();
        let invite = ClusterInvite::generate("test-cluster", &ca, ClusterRole::Operator).unwrap();
        assert!(!invite.code.is_empty());
        assert!(invite.pem_bundle.contains("BEGIN CERTIFICATE"));
        assert!(invite.pem_bundle.contains("BEGIN PRIVATE KEY"));
        let identity = invite.to_identity().unwrap();
        assert_eq!(identity.role(), ClusterRole::Operator);
    }

    #[test]
    fn test_invite_code_format() {
        let code = ClusterInvite::generate_code();
        assert_eq!(code.len(), 12);
        assert!(code.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
