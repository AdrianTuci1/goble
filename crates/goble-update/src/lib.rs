//! Update delivery for Goble.
//!
//! # The shape of it
//!
//! One JSON manifest per release lists, per channel, the version and the
//! artifacts for each platform with their SHA-256 digests. The client fetches
//! that manifest, compares the version against its own, downloads the artifact
//! for its platform into a staging directory, checks the digest, and then hands
//! it to a platform-specific install plan.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use goble_update::{Channel, UpdateClient, UpdateConfig, install};
//!
//! let config = UpdateConfig::new("https://releases.goble.dev/channel_versions.json")
//!     .with_public_key("…hex ed25519 public key…");
//! let client = UpdateClient::new(config, "goble/0.1.0")?;
//!
//! if let Some(update) = client
//!     .check(Channel::Stable, "0.1.0", std::env::consts::OS, std::env::consts::ARCH)?
//!     .into_update()
//! {
//!     let staging = std::path::Path::new("/tmp/goble-updates");
//!     let staged = client.download(&update, staging)?;
//!     let plan = install::plan_install(&install::InstallContext::detect(), &staged)?;
//!     println!("{}", plan.description());
//!     # let _ = plan;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # What this does not do
//!
//! - No background polling, no "check on window activation". The app decides
//!   when to ask; see `app/src/main.rs`.
//! - No forced updates and no version pinning. A release is offered, never
//!   imposed.
//! - No code copying from other terminals: the manifest-per-channel idea is the
//!   same one every desktop app has, and the interesting parts here (signature
//!   verification, refusing an unsigned release, handing package-managed
//!   installs back to the package manager) are decisions, not borrowings.
//!
//! # Trust
//!
//! The digest protects the download against corruption. It cannot protect
//! against a hostile manifest, because the digest travels inside the manifest.
//! Only the signature does that, so a build with no configured public key
//! refuses to install anything unless it was explicitly told to allow unsigned
//! releases.

pub mod client;
pub mod install;
pub mod manifest;

pub use client::{
    AvailableUpdate, CheckOutcome, StagedArtifact, StagingWriter, UpdateClient, UpdateConfig,
    UpdateError, parse_version,
};
pub use install::{InstallContext, InstallError, InstallOutcome, InstallPlan};
pub use manifest::{
    Artifact, Channel, ChannelManifest, ChannelRelease, Channels, ManifestError, SCHEMA,
};

impl CheckOutcome {
    /// The update, when there is one.
    pub fn into_update(self) -> Option<AvailableUpdate> {
        match self {
            Self::Available(update) => Some(*update),
            Self::UpToDate { .. } | Self::Unsupported { .. } => None,
        }
    }

    /// Whether this build can act on the answer at all.
    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unsupported { .. })
    }
}
