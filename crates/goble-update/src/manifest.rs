//! The update manifest: what a release publishes, and what a client trusts.
//!
//! One JSON document per release, served from the release host:
//!
//! ```json
//! {
//!   "schema": 1,
//!   "channels": {
//!     "stable": {
//!       "version": "0.2.0",
//!       "released_at": "2026-09-01T10:00:00Z",
//!       "notes_url": "https://goble.dev/releases/0.2.0",
//!       "artifacts": [
//!         {"os": "macos", "arch": "aarch64", "url": "https://…/Goble-0.2.0-arm64.dmg",
//!          "sha256": "…", "size": 12345678}
//!       ],
//!       "signature": "…"
//!     }
//!   }
//! }
//! ```
//!
//! The digest is checked for every download. The signature is checked for the
//! manifest itself: without it, whoever controls the release host — or its TLS
//! termination, or a DNS answer — decides what every installation runs. The
//! digest alone cannot help, because it travels in the same document.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Manifest format this build understands.
pub const SCHEMA: u32 = 1;

/// An artifact that runs on every architecture of its platform.
pub const UNIVERSAL: &str = "universal";

/// The prefix of the bytes a signature covers. Changing the payload shape means
/// changing this string, so an old signature can never be replayed into a new
/// payload shape.
const SIGNING_DOMAIN: &str = "goble-update-v1";

/// Which release line a build follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
    Dev,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Dev => "dev",
        }
    }

    /// Whether this channel's builds are allowed to replace themselves. A dev
    /// build is run from a working tree or installed by hand; it has no claim on
    /// the user's installation directory.
    pub fn is_self_updating(self) -> bool {
        !matches!(self, Self::Dev)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// `macos`, `linux` or `windows` — the values of `std::env::consts::OS`.
    pub os: String,
    /// `aarch64`, `x86_64` or `universal`.
    pub arch: String,
    pub url: String,
    /// Lowercase hex SHA-256 of the artifact.
    pub sha256: String,
    pub size: u64,
}

impl Artifact {
    /// Whether this artifact is offered for a platform.
    pub fn matches(&self, os: &str, arch: &str) -> bool {
        self.os.eq_ignore_ascii_case(os)
            && (self.arch.eq_ignore_ascii_case(arch) || self.arch.eq_ignore_ascii_case(UNIVERSAL))
    }

    /// A `universal` artifact is a fallback, not a preference.
    fn is_universal(&self) -> bool {
        self.arch.eq_ignore_ascii_case(UNIVERSAL)
    }
}

/// Everything published for one channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRelease {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    /// Hex ed25519 signature over [`Self::signing_payload`]. Absent means
    /// unsigned, which an installer must refuse unless it was told otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ChannelRelease {
    /// The exact bytes a signature covers.
    ///
    /// Deliberately not "the JSON": JSON has many spellings for the same value,
    /// and a signature over a serialisaion is a signature over whoever happened
    /// to write the serializer. Artifacts are sorted so the order in the file
    /// does not change the payload.
    pub fn signing_payload(&self) -> String {
        let mut lines = vec![SIGNING_DOMAIN.to_owned(), self.version.clone()];
        let mut artifacts: Vec<String> = self
            .artifacts
            .iter()
            .map(|artifact| {
                format!(
                    "{} {} {} {} {}",
                    artifact.os.to_ascii_lowercase(),
                    artifact.arch.to_ascii_lowercase(),
                    artifact.sha256.to_ascii_lowercase(),
                    artifact.size,
                    artifact.url
                )
            })
            .collect();
        artifacts.sort();
        lines.extend(artifacts);
        lines.join("\n")
    }

    /// The artifact for a platform: an exact architecture match wins over a
    /// universal build.
    pub fn artifact_for(&self, os: &str, arch: &str) -> Option<&Artifact> {
        let candidates: Vec<&Artifact> =
            self.artifacts.iter().filter(|artifact| artifact.matches(os, arch)).collect();
        candidates.iter().find(|artifact| !artifact.is_universal()).copied().or_else(|| {
            candidates.first().copied()
        })
    }

    /// Check everything that can be checked without a network.
    pub fn validate(&self) -> Result<(), ManifestError> {
        semver::Version::parse(&self.version).map_err(|_| ManifestError::BadVersion {
            version: self.version.clone(),
        })?;

        if self.artifacts.is_empty() {
            return Err(ManifestError::NoArtifacts);
        }

        for artifact in &self.artifacts {
            if artifact.sha256.len() != 64 || !artifact.sha256.chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err(ManifestError::BadDigest {
                    url: artifact.url.clone(),
                    digest: artifact.sha256.clone(),
                });
            }
            if artifact.size == 0 {
                return Err(ManifestError::EmptyArtifact { url: artifact.url.clone() });
            }
            if !is_acceptable_url(&artifact.url) {
                return Err(ManifestError::InsecureUrl { url: artifact.url.clone() });
            }
        }
        Ok(())
    }

    /// Verify the manifest signature against a hex ed25519 public key.
    pub fn verify_signature(&self, public_key_hex: &str) -> Result<(), ManifestError> {
        let Some(signature) = self.signature.as_deref() else {
            return Err(ManifestError::MissingSignature);
        };

        let key = hex::decode(public_key_hex.trim())
            .map_err(|_| ManifestError::BadPublicKey { reason: "not hex".to_owned() })?;
        let signature = hex::decode(signature.trim())
            .map_err(|_| ManifestError::BadSignature { reason: "not hex".to_owned() })?;

        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
            .verify(self.signing_payload().as_bytes(), &signature)
            .map_err(|_| ManifestError::BadSignature {
                reason: "the signature does not match the release".to_owned(),
            })
    }
}

/// The whole document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelManifest {
    pub schema: u32,
    #[serde(default)]
    pub channels: Channels,
}

/// Channels are a fixed set, so a typo in a manifest is a parse problem rather
/// than a channel nobody ever fetches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable: Option<ChannelRelease>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beta: Option<ChannelRelease>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<ChannelRelease>,
}

impl Channels {
    pub fn get(&self, channel: Channel) -> Option<&ChannelRelease> {
        match channel {
            Channel::Stable => self.stable.as_ref(),
            Channel::Beta => self.beta.as_ref(),
            Channel::Dev => self.dev.as_ref(),
        }
    }
}

impl ChannelManifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Self =
            serde_json::from_str(text).map_err(|err| ManifestError::Json { reason: err.to_string() })?;
        if manifest.schema != SCHEMA {
            return Err(ManifestError::UnsupportedSchema { found: manifest.schema, supported: SCHEMA });
        }
        Ok(manifest)
    }

    /// The release for a channel, validated.
    pub fn release(&self, channel: Channel) -> Result<&ChannelRelease, ManifestError> {
        let release = self
            .channels
            .get(channel)
            .ok_or(ManifestError::NoRelease { channel: channel.as_str() })?;
        release.validate()?;
        Ok(release)
    }

    /// Parse, validate the requested channel, and verify its signature.
    pub fn trusted_release(
        &self,
        channel: Channel,
        public_key_hex: &str,
    ) -> Result<&ChannelRelease, ManifestError> {
        let release = self.release(channel)?;
        release.verify_signature(public_key_hex)?;
        Ok(release)
    }
}

/// A manifest in a shape that cannot be used.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the manifest is not valid JSON: {reason}")]
    Json { reason: String },
    #[error("manifest schema {found} is not supported (this build speaks {supported})")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("the manifest has no `{channel}` channel")]
    NoRelease { channel: &'static str },
    #[error("release version `{version}` is not a semver version")]
    BadVersion { version: String },
    #[error("a release with no artifacts cannot be installed")]
    NoArtifacts,
    #[error("{url} has a digest that is not 64 hex characters: {digest}")]
    BadDigest { url: String, digest: String },
    #[error("{url} declares no size")]
    EmptyArtifact { url: String },
    #[error("{url} is not https")]
    InsecureUrl { url: String },
    #[error("the release is not signed")]
    MissingSignature,
    #[error("the public key is unusable: {reason}")]
    BadPublicKey { reason: String },
    #[error("the signature is unusable: {reason}")]
    BadSignature { reason: String },
}

/// Downloads must be https, except from the loopback interface, which is how a
/// release is smoke-tested on a developer's machine.
fn is_acceptable_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")) else {
        return false;
    };
    if url.starts_with("https://") {
        return true;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).trim_matches(['[', ']']);
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0")
}

/// Group artifacts by platform, for a release page that has to show them.
pub fn artifacts_by_platform(release: &ChannelRelease) -> BTreeMap<String, Vec<&Artifact>> {
    let mut grouped: BTreeMap<String, Vec<&Artifact>> = BTreeMap::new();
    for artifact in &release.artifacts {
        grouped.entry(artifact.os.to_ascii_lowercase()).or_default().push(artifact);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair as _;

    fn artifact(os: &str, arch: &str) -> Artifact {
        Artifact {
            os: os.into(),
            arch: arch.into(),
            url: format!("https://releases.goble.dev/stable/0.2.0/Goble-{os}-{arch}"),
            sha256: "a".repeat(64),
            size: 1024,
        }
    }

    fn sample() -> ChannelRelease {
        ChannelRelease {
            version: "0.2.0".into(),
            released_at: None,
            notes_url: None,
            artifacts: vec![
                artifact("macos", "universal"),
                artifact("macos", "aarch64"),
                artifact("linux", "x86_64"),
                artifact("windows", "x86_64"),
            ],
            signature: None,
        }
    }

    fn manifest_text() -> String {
        serde_json::json!({
            "schema": SCHEMA,
            "channels": { "stable": sample() }
        })
        .to_string()
    }

    #[test]
    fn a_manifest_parses_and_validates() {
        let manifest = ChannelManifest::parse(&manifest_text()).unwrap();
        let release = manifest.release(Channel::Stable).unwrap();
        assert_eq!(release.version, "0.2.0");
        assert_eq!(release.artifacts.len(), 4);
    }

    #[test]
    fn an_unknown_schema_is_refused() {
        let text = serde_json::json!({"schema": 99, "channels": {}}).to_string();
        assert_eq!(
            ChannelManifest::parse(&text),
            Err(ManifestError::UnsupportedSchema { found: 99, supported: SCHEMA })
        );
    }

    #[test]
    fn a_missing_channel_is_refused() {
        let manifest = ChannelManifest::parse(&manifest_text()).unwrap();
        assert_eq!(
            manifest.release(Channel::Beta),
            Err(ManifestError::NoRelease { channel: "beta" })
        );
    }

    #[test]
    fn a_version_that_is_not_semver_is_refused() {
        let mut release = sample();
        release.version = "nightly-2026-09-01".into();
        assert_eq!(
            release.validate(),
            Err(ManifestError::BadVersion { version: "nightly-2026-09-01".into() })
        );
    }

    #[test]
    fn an_artifact_without_a_real_digest_is_refused() {
        let mut release = sample();
        release.artifacts[0].sha256 = "tooshort".into();
        assert!(matches!(release.validate(), Err(ManifestError::BadDigest { .. })));

        let mut release = sample();
        release.artifacts[0].sha256 = "z".repeat(64);
        assert!(matches!(release.validate(), Err(ManifestError::BadDigest { .. })));
    }

    #[test]
    fn an_artifact_without_a_size_is_refused() {
        let mut release = sample();
        release.artifacts[0].size = 0;
        assert!(matches!(release.validate(), Err(ManifestError::EmptyArtifact { .. })));
    }

    #[test]
    fn a_release_without_artifacts_is_refused() {
        let mut release = sample();
        release.artifacts.clear();
        assert_eq!(release.validate(), Err(ManifestError::NoArtifacts));
    }

    #[test]
    fn plain_http_is_refused_except_on_the_loopback() {
        let mut release = sample();
        release.artifacts[0].url = "http://releases.goble.dev/x".into();
        assert!(matches!(release.validate(), Err(ManifestError::InsecureUrl { .. })));

        release.artifacts[0].url = "http://127.0.0.1:8080/x".into();
        assert_eq!(release.validate(), Ok(()));

        release.artifacts[0].url = "ftp://releases.goble.dev/x".into();
        assert!(matches!(release.validate(), Err(ManifestError::InsecureUrl { .. })));
    }

    #[test]
    fn an_exact_architecture_wins_over_a_universal_build() {
        let release = sample();
        assert_eq!(release.artifact_for("macos", "aarch64").unwrap().arch, "aarch64");
        assert_eq!(release.artifact_for("macos", "x86_64").unwrap().arch, "universal");
        assert!(release.artifact_for("linux", "aarch64").is_none());
    }

    #[test]
    fn the_signing_payload_ignores_the_order_of_artifacts() {
        let mut shuffled = sample();
        shuffled.artifacts.reverse();
        assert_eq!(sample().signing_payload(), shuffled.signing_payload());
    }

    #[test]
    fn the_signing_payload_changes_when_an_artifact_changes() {
        let mut tampered = sample();
        tampered.artifacts[0].sha256 = "b".repeat(64);
        assert_ne!(sample().signing_payload(), tampered.signing_payload());
    }

    #[test]
    fn a_real_signature_verifies_and_a_tampered_release_does_not() {
        // A throwaway key pair, generated in the test: this checks the wiring,
        // not any particular release key.
        let seed = [7_u8; 32];
        let key_pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let public_key = hex::encode(key_pair.public_key().as_ref());

        let mut signed = sample();
        signed.signature = Some(hex::encode(key_pair.sign(signed.signing_payload().as_bytes())));
        assert_eq!(signed.verify_signature(&public_key), Ok(()));

        let mut tampered = signed.clone();
        tampered.artifacts[0].sha256 = "c".repeat(64);
        assert!(matches!(
            tampered.verify_signature(&public_key),
            Err(ManifestError::BadSignature { .. })
        ));

        let mut unsigned = signed.clone();
        unsigned.signature = None;
        assert_eq!(unsigned.verify_signature(&public_key), Err(ManifestError::MissingSignature));
    }

    #[test]
    fn a_signature_from_a_different_key_is_refused() {
        let signer = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).unwrap();
        let other = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9_u8; 32]).unwrap();

        let mut signed = sample();
        signed.signature = Some(hex::encode(signer.sign(signed.signing_payload().as_bytes())));

        let other_public = hex::encode(other.public_key().as_ref());
        assert!(matches!(
            signed.verify_signature(&other_public),
            Err(ManifestError::BadSignature { .. })
        ));
    }

    #[test]
    fn a_bad_public_key_is_reported_as_such() {
        let mut signed = sample();
        signed.signature = Some("00".into());
        assert!(matches!(
            signed.verify_signature("not hex"),
            Err(ManifestError::BadPublicKey { .. })
        ));
    }

    #[test]
    fn a_trusted_release_needs_both_validation_and_a_signature() {
        let key_pair =
            ring::signature::Ed25519KeyPair::from_seed_unchecked(&[3_u8; 32]).unwrap();
        let public_key = hex::encode(key_pair.public_key().as_ref());

        let mut signed = sample();
        signed.signature = Some(hex::encode(key_pair.sign(signed.signing_payload().as_bytes())));

        let text =
            serde_json::json!({"schema": SCHEMA, "channels": {"stable": signed}}).to_string();
        let manifest = ChannelManifest::parse(&text).unwrap();
        assert!(manifest.trusted_release(Channel::Stable, &public_key).is_ok());

        // Unsigned manifests are refused by the same call.
        let text = manifest_text();
        let manifest = ChannelManifest::parse(&text).unwrap();
        assert_eq!(
            manifest.trusted_release(Channel::Stable, &public_key),
            Err(ManifestError::MissingSignature)
        );
    }

    #[test]
    fn artifacts_group_by_platform() {
        let release = sample();
        let grouped = artifacts_by_platform(&release);
        assert_eq!(grouped["macos"].len(), 2);
        assert_eq!(grouped["linux"].len(), 1);
        assert!(grouped["windows"].len() == 1);
    }

    #[test]
    fn only_dev_builds_decline_to_update_themselves() {
        assert!(Channel::Stable.is_self_updating());
        assert!(Channel::Beta.is_self_updating());
        assert!(!Channel::Dev.is_self_updating());
    }
}
