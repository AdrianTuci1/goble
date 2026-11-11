//! The relay's identity and transport layer.
//!
//! The relay forwards opaque frames. This crate owns the parts that decide who
//! a peer is and what the relay may read:
//!
//! - [`pairing`] — a two-channel QR + SAS ceremony that issues a device
//!   credential over a channel the relay cannot open.
//! - [`passkey`] — WebAuthn registration and assertion verification for the
//!   browser session gate.
//! - [`auth`] — short access tokens, rotating refresh tokens and step-up
//!   approvals.
//! - [`crypto`] — X25519 + HKDF + ChaCha20-Poly1305 for end-to-end frames.
//! - [`registry`] — durable device, worker and revocation records.
//!
//! Nothing here parses daemon frames. A relay node uses these modules to
//! authenticate a connection and then treats its payload as bytes.

pub mod auth;
pub mod crypto;
pub mod pairing;
pub mod passkey;
pub mod registry;

pub use crypto::{DirectionalCipher, EphemeralSecret, Session};
pub use pairing::{OfferBook, PairingInvite, PairingOffer};
pub use registry::{DeviceRecord, Registry, WorkerRecord};
