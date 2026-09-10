//! Fetching the manifest, deciding whether an update exists, and staging the
//! artifact.
//!
//! Nothing here writes outside the staging directory, and nothing is ever
//! handed to an installer before its digest has been checked against the
//! signed manifest.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use semver::Version;
use sha2::{Digest, Sha256};

use crate::manifest::{Artifact, Channel, ChannelManifest, ChannelRelease, ManifestError};

/// Why an update could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not reach {url}: {reason}")]
    Transport { url: String, reason: String },
    #[error("{url} answered {status}")]
    Status { url: String, status: u16 },
    #[error("the release manifest is unusable: {0}")]
    Manifest(#[from] ManifestError),
    #[error("the running version `{version}` is not a semver version, so it cannot be compared")]
    UncomparableVersion { version: String },
    #[error("{url} is larger than the {limit} bytes this build will accept")]
    TooLarge { url: String, limit: u64 },
    #[error("{url} does not match its declared SHA-256 (expected {expected}, got {actual})")]
    DigestMismatch { url: String, expected: String, actual: String },
    #[error("{url} is {actual} bytes, but the manifest declares {expected}")]
    SizeMismatch { url: String, expected: u64, actual: u64 },
    #[error("the release is not signed, and this build is not allowed to install unsigned releases")]
    Untrusted,
    #[error("{path} could not be prepared: {reason}")]
    Io { path: PathBuf, reason: String },
}

/// A staged, verified artifact on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedArtifact {
    pub path: PathBuf,
    pub artifact: Artifact,
    /// The digest that was actually computed, not the one that was promised.
    pub digest: String,
}

impl StagedArtifact {
    /// File name as published, e.g. `Goble-0.2.0-arm64.dmg`.
    pub fn file_name(&self) -> String {
        self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// A release that can be installed on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub channel: Channel,
    pub version: Version,
    pub artifact: Artifact,
    pub notes_url: Option<String>,
    pub released_at: Option<DateTime<Utc>>,
    /// Whether the manifest signature was checked against a known key.
    pub verified: bool,
}

impl AvailableUpdate {
    pub fn version_string(&self) -> String {
        self.version.to_string()
    }
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    UpToDate { current: Version, latest: Version },
    Available(Box<AvailableUpdate>),
    /// This build should not be replacing itself at all.
    Unsupported { reason: String },
}

/// How the client behaves.
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    /// URL of the release manifest.
    pub manifest_url: String,
    /// Hex ed25519 public key the manifest must be signed with. Without it,
    /// `check` reports releases as unverified and staging refuses them.
    pub public_key: Option<String>,
    /// Allow installing an unverified release. Development only: it turns
    /// whoever controls the release host into whoever controls the user's
    /// machine.
    pub allow_unsigned: bool,
    pub request_timeout: Duration,
    /// Refuse to download anything larger than this.
    pub max_artifact_bytes: u64,
}

impl UpdateConfig {
    pub fn new(manifest_url: impl Into<String>) -> Self {
        Self {
            manifest_url: manifest_url.into(),
            public_key: None,
            allow_unsigned: false,
            request_timeout: Duration::from_secs(30),
            max_artifact_bytes: 512 * 1024 * 1024,
        }
    }

    pub fn with_public_key(mut self, key: impl Into<String>) -> Self {
        self.public_key = Some(key.into());
        self
    }
}

/// Talks to the release host.
pub struct UpdateClient {
    runtime: tokio::runtime::Runtime,
    http: reqwest::Client,
    config: UpdateConfig,
}

impl std::fmt::Debug for UpdateClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateClient")
            .field("manifest_url", &self.config.manifest_url)
            .field("signed", &self.config.public_key.is_some())
            .finish()
    }
}

impl UpdateClient {
    pub fn new(config: UpdateConfig, user_agent: impl Into<String>) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let http = reqwest::Client::builder()
            .user_agent(user_agent.into())
            .timeout(config.request_timeout)
            .build()
            .map_err(std::io::Error::other)?;
        Ok(Self { runtime, http, config })
    }

    pub fn config(&self) -> &UpdateConfig {
        &self.config
    }

    /// Fetch the manifest document.
    pub fn fetch_manifest(&self) -> Result<ChannelManifest, UpdateError> {
        let url = self.config.manifest_url.clone();
        let text = self.get_text(&url)?;
        ChannelManifest::parse(&text).map_err(UpdateError::from)
    }

    /// Decide whether this build should be replaced.
    ///
    /// The manifest is validated, and its signature is checked when a key is
    /// configured. Without a key the outcome is reported but marked unverified,
    /// and [`Self::download`] will refuse it.
    pub fn check(
        &self,
        channel: Channel,
        current_version: &str,
        os: &str,
        arch: &str,
    ) -> Result<CheckOutcome, UpdateError> {
        if !channel.is_self_updating() {
            return Ok(CheckOutcome::Unsupported {
                reason: format!("a {} build does not update itself", channel.as_str()),
            });
        }

        let current = parse_version(current_version)?;
        let manifest = self.fetch_manifest()?;
        let release = manifest.release(channel)?;
        let latest = parse_version(&release.version)?;

        if latest <= current {
            return Ok(CheckOutcome::UpToDate { current, latest });
        }

        let Some(artifact) = release.artifact_for(os, arch) else {
            return Ok(CheckOutcome::Unsupported {
                reason: format!(
                    "release {} has no artifact for {os}/{arch}",
                    release.version
                ),
            });
        };

        let verified = self.release_is_signed(release)?;
        Ok(CheckOutcome::Available(Box::new(AvailableUpdate {
            channel,
            version: latest,
            artifact: artifact.clone(),
            notes_url: release.notes_url.clone(),
            released_at: release.released_at,
            verified,
        })))
    }

    /// Whether the manifest signature checks out against the configured key.
    fn release_is_signed(&self, release: &ChannelRelease) -> Result<bool, UpdateError> {
        match &self.config.public_key {
            Some(key) => {
                release.verify_signature(key)?;
                Ok(true)
            }
            None => {
                log::warn!(
                    "no update public key is configured: the release is not authenticated"
                );
                Ok(false)
            }
        }
    }

    /// Download an update into `dir`, verifying it before it is kept.
    pub fn download(
        &self,
        update: &AvailableUpdate,
        dir: &Path,
    ) -> Result<StagedArtifact, UpdateError> {
        if !update.verified && !self.config.allow_unsigned {
            return Err(UpdateError::Untrusted);
        }

        std::fs::create_dir_all(dir).map_err(|err| io_error(dir, err))?;
        let url = update.artifact.url.clone();
        let artifact = update.artifact.clone();
        let limit = self.config.max_artifact_bytes;

        let client = self.http.clone();
        self.runtime.block_on(async move {
            let mut response = client
                .get(&url)
                .send()
                .await
                .map_err(|err| UpdateError::Transport { url: url.clone(), reason: err.to_string() })?;

            if !response.status().is_success() {
                return Err(UpdateError::Status { url: url.clone(), status: response.status().as_u16() });
            }

            let mut staging = StagingWriter::create(dir, &artifact, limit)?;
            loop {
                let chunk = response.chunk().await.map_err(|err| UpdateError::Transport {
                    url: url.clone(),
                    reason: err.to_string(),
                })?;
                let Some(chunk) = chunk else {
                    break;
                };
                staging.write(&chunk)?;
            }
            staging.finish()
        })
    }

    fn get_text(&self, url: &str) -> Result<String, UpdateError> {
        let client = self.http.clone();
        let url = url.to_owned();
        self.runtime.block_on(async move {
            let response = client.get(&url).send().await.map_err(|err| UpdateError::Transport {
                url: url.clone(),
                reason: err.to_string(),
            })?;
            if !response.status().is_success() {
                return Err(UpdateError::Status { url: url.clone(), status: response.status().as_u16() });
            }
            response.text().await.map_err(|err| UpdateError::Transport {
                url: url.clone(),
                reason: err.to_string(),
            })
        })
    }
}

/// Writes an artifact to disk while hashing it, and only keeps it if the
/// digest and the size are what the manifest promised.
#[derive(Debug)]
pub struct StagingWriter {
    part: PathBuf,
    file: std::fs::File,
    hasher: Sha256,
    written: u64,
    limit: u64,
    artifact: Artifact,
}

impl StagingWriter {
    pub fn create(dir: &Path, artifact: &Artifact, limit: u64) -> Result<Self, UpdateError> {
        std::fs::create_dir_all(dir).map_err(|err| io_error(dir, err))?;
        let part = dir.join(format!("{}.part", file_name_of(&artifact.url)));
        let file = std::fs::File::create(&part).map_err(|err| io_error(&part, err))?;
        Ok(Self {
            part,
            file,
            hasher: Sha256::new(),
            written: 0,
            limit,
            artifact: artifact.clone(),
        })
    }

    pub fn write(&mut self, chunk: &[u8]) -> Result<(), UpdateError> {
        self.written += chunk.len() as u64;
        if self.written > self.limit {
            let _ = std::fs::remove_file(&self.part);
            return Err(UpdateError::TooLarge { url: self.artifact.url.clone(), limit: self.limit });
        }
        self.hasher.update(chunk);
        self.file
            .write_all(chunk)
            .map_err(|err| io_error(&self.part, err))
    }

    /// Check what was written, then move it into place.
    pub fn finish(self) -> Result<StagedArtifact, UpdateError> {
        let digest = hex::encode(self.hasher.finalize());
        drop(self.file);

        if self.written != self.artifact.size {
            let _ = std::fs::remove_file(&self.part);
            return Err(UpdateError::SizeMismatch {
                url: self.artifact.url.clone(),
                expected: self.artifact.size,
                actual: self.written,
            });
        }

        let expected = self.artifact.sha256.to_ascii_lowercase();
        if digest != expected {
            let _ = std::fs::remove_file(&self.part);
            return Err(UpdateError::DigestMismatch {
                url: self.artifact.url.clone(),
                expected,
                actual: digest,
            });
        }

        let path = self.part.with_extension("");
        std::fs::rename(&self.part, &path).map_err(|err| io_error(&path, err))?;
        make_executable(&path)?;

        Ok(StagedArtifact { path, artifact: self.artifact, digest })
    }
}

/// `1.2.3`, `v1.2.3` and `1.2.3-beta.1` all parse.
pub fn parse_version(text: &str) -> Result<Version, UpdateError> {
    let trimmed = text.trim().trim_start_matches('v');
    Version::parse(trimmed)
        .map_err(|_| UpdateError::UncomparableVersion { version: text.to_owned() })
}

fn file_name_of(url: &str) -> String {
    // Query and fragment first, then the last path segment; a name taken the
    // other way round would turn `x.exe?v=2` into `v2`.
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("artifact");
    let cleaned: String =
        last.chars().filter(|c| c.is_ascii_alphanumeric() || "._-+".contains(*c)).collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        "artifact".to_owned()
    } else {
        cleaned
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions =
        std::fs::metadata(path).map_err(|err| io_error(path, err))?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).map_err(|err| io_error(path, err))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

fn io_error(path: &Path, err: std::io::Error) -> UpdateError {
    UpdateError::Io { path: path.to_path_buf(), reason: err.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(payload: &[u8]) -> Artifact {
        Artifact {
            os: "macos".into(),
            arch: "aarch64".into(),
            url: "https://releases.goble.dev/stable/0.2.0/Goble-0.2.0-arm64.dmg".into(),
            sha256: hex::encode(Sha256::digest(payload)),
            size: payload.len() as u64,
        }
    }

    fn stage(dir: &Path, payload: &[u8], artifact: &Artifact) -> Result<StagedArtifact, UpdateError> {
        let mut writer = StagingWriter::create(dir, artifact, 1024 * 1024)?;
        // Two chunks, because that is how a real download arrives.
        let (first, second) = payload.split_at(payload.len() / 2);
        writer.write(first)?;
        writer.write(second)?;
        writer.finish()
    }

    #[test]
    fn a_matching_artifact_is_staged_and_left_executable() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"a release".repeat(500);
        let artifact = artifact(&payload);

        let staged = stage(dir.path(), &payload, &artifact).unwrap();
        assert!(staged.path.is_file());
        assert_eq!(staged.digest, artifact.sha256);
        assert!(staged.file_name().ends_with(".dmg"), "{}", staged.file_name());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&staged.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "an app image has to be runnable");
        }

        // No `.part` file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "part"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn a_tampered_artifact_is_refused_and_not_left_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"a release".repeat(100);
        let mut artifact = artifact(&payload);
        artifact.sha256 = "0".repeat(64);

        let error = stage(dir.path(), &payload, &artifact).unwrap_err();
        assert!(matches!(error, UpdateError::DigestMismatch { .. }), "{error}");
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing may be kept");
    }

    #[test]
    fn an_artifact_of_the_wrong_size_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"a release".repeat(100);
        let mut artifact = artifact(&payload);
        artifact.size += 1;
        // The digest is computed over the payload, so the size is the only lie.
        let error = stage(dir.path(), &payload, &artifact).unwrap_err();
        assert!(matches!(error, UpdateError::SizeMismatch { .. }), "{error}");
    }

    #[test]
    fn an_oversized_artifact_is_cut_off_mid_download() {
        let dir = tempfile::tempdir().unwrap();
        let payload = vec![0_u8; 4096];
        let artifact = artifact(&payload);

        let mut writer = StagingWriter::create(dir.path(), &artifact, 1024).unwrap();
        let error = writer.write(&payload).unwrap_err();
        assert!(matches!(error, UpdateError::TooLarge { .. }), "{error}");
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn a_version_tag_is_parsed_with_or_without_its_prefix() {
        assert_eq!(parse_version("0.2.0").unwrap(), Version::new(0, 2, 0));
        assert_eq!(parse_version("v0.2.0").unwrap(), Version::new(0, 2, 0));
        assert_eq!(
            parse_version(" 0.2.0-beta.1 ").unwrap(),
            Version::parse("0.2.0-beta.1").unwrap()
        );
        assert!(matches!(
            parse_version("nightly"),
            Err(UpdateError::UncomparableVersion { .. })
        ));
    }

    #[test]
    fn a_prerelease_is_older_than_its_release() {
        assert!(parse_version("0.2.0-beta.1").unwrap() < parse_version("0.2.0").unwrap());
        assert!(parse_version("0.2.0").unwrap() < parse_version("0.2.1").unwrap());
    }

    #[test]
    fn an_unsigned_update_is_refused_by_default() {
        let update = AvailableUpdate {
            channel: Channel::Stable,
            version: Version::new(0, 2, 0),
            artifact: artifact(b"x"),
            notes_url: None,
            released_at: None,
            verified: false,
        };
        let config = UpdateConfig::new("https://releases.goble.dev/channel_versions.json");
        let client = UpdateClient::new(config, "goble-test").unwrap();
        let dir = tempfile::tempdir().unwrap();

        let error = client.download(&update, dir.path()).unwrap_err();
        assert!(matches!(error, UpdateError::Untrusted), "{error}");
    }

    #[test]
    fn a_file_name_is_taken_from_the_url_and_cleaned() {
        assert_eq!(file_name_of("https://host/a/Goble-0.2.0-arm64.dmg"), "Goble-0.2.0-arm64.dmg");
        assert_eq!(file_name_of("https://host/a/x.exe?v=2"), "x.exe");
        assert_eq!(file_name_of("https://host/a/"), "artifact");
        // A path in a URL can never reach outside the staging directory.
        assert_eq!(file_name_of("https://host/../../etc/passwd"), "passwd");
        let escaped = file_name_of("https://host/a/..%2f..%2fetc");
        assert!(!escaped.contains('/') && !escaped.contains('\\'), "{escaped}");
    }
}
