use std::fmt;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rcgen::CustomExtension;
use serde::{Deserialize, Serialize};
use x509_parser::prelude::*;

use super::pem::{offset_to_chrono, parse_utf8_der, pem_to_der};

/// String form of the Goble role extension OID.
const GOBLE_ROLE_OID_STR: &str = "1.3.6.1.4.1.42069.100.1.1";
/// Arc components of the Goble role extension OID.
pub const GOBLE_ROLE_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 42069, 100, 1, 1];

/// Hierarchical role embedded in every cluster certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ClusterRole {
    Owner,
    Admin,
    Operator,
    Viewer,
    Worker,
}

impl ClusterRole {
    pub fn is_device(self) -> bool {
        !matches!(self, Self::Worker)
    }

    pub fn is_worker(self) -> bool {
        matches!(self, Self::Worker)
    }

    pub fn can_manage_cluster(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    pub fn can_operate(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Operator)
    }
}

impl fmt::Display for ClusterRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Owner => "Owner",
            Self::Admin => "Admin",
            Self::Operator => "Operator",
            Self::Viewer => "Viewer",
            Self::Worker => "Worker",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for ClusterRole {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Owner" => Ok(Self::Owner),
            "Admin" => Ok(Self::Admin),
            "Operator" => Ok(Self::Operator),
            "Viewer" => Ok(Self::Viewer),
            "Worker" => Ok(Self::Worker),
            _ => anyhow::bail!("unknown cluster role: {}", s),
        }
    }
}

/// Non-critical X.509 custom extension that carries the certificate role.
pub(super) struct RoleExtension;

impl RoleExtension {
    pub(super) fn make(role: ClusterRole) -> CustomExtension {
        let content = yasna::construct_der(|writer| {
            writer.write_utf8string(role.to_string().as_str());
        });
        CustomExtension::from_oid_content(GOBLE_ROLE_OID, content)
    }
}

/// Extract the Goble role from a PEM certificate.
pub fn extract_role(cert_pem: &str) -> Result<ClusterRole> {
    let der = pem_to_der(cert_pem)?;
    let (_, x509) = X509Certificate::from_der(&der)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;
    extract_role_from_cert(&x509)
}

pub(super) fn extract_role_from_cert(x509: &X509Certificate<'_>) -> Result<ClusterRole> {
    for ext in x509.extensions() {
        if ext.oid.to_id_string() == GOBLE_ROLE_OID_STR {
            return parse_utf8_der(ext.value)
                .and_then(|s| s.parse::<ClusterRole>())
                .context("invalid role extension content");
        }
    }
    anyhow::bail!("certificate does not contain a Goble role extension")
}

/// Extract the serial number as a lowercase hex string.
pub fn extract_serial(cert_pem: &str) -> Result<String> {
    let der = pem_to_der(cert_pem)?;
    let (_, x509) = X509Certificate::from_der(&der)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;
    Ok(hex::encode(x509.serial.to_bytes_be()))
}

/// Extract certificate validity window.
pub fn extract_validity(cert_pem: &str) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let der = pem_to_der(cert_pem)?;
    let (_, x509) = X509Certificate::from_der(&der)
        .map_err(|e| anyhow::anyhow!("failed to parse certificate: {e}"))?;
    let not_before = x509.validity.not_before.to_datetime();
    let not_after = x509.validity.not_after.to_datetime();
    Ok((offset_to_chrono(not_before), offset_to_chrono(not_after)))
}
