//! Cluster identity: the role extension, the certificate and CRL material and
//! the rustls verifiers that enforce them.
//!
//! One module per surface: the role and the certificate field extractors
//! ([`role`]), the PEM/DER helpers ([`pem`]), an issued [`Identity`]
//! ([`material`]), CRL signing and verification ([`crl`]), the in-memory
//! certificate store ([`certs`]), the rustls verifiers ([`verifier`]) and the
//! cluster CA that issues everything ([`ca`]).
//!
//! Every certificate carries the Goble role extension (OID
//! `1.3.6.1.4.1.42069.100.1.1`), so a peer's role is read from the certificate
//! itself: the verifiers validate the chain with webpki, then require the role
//! the connection is for and honor the revoked serials they were given.

mod ca;
mod certs;
mod crl;
mod material;
mod pem;
mod role;
mod verifier;

#[cfg(test)]
mod tests;

pub use ca::ClusterCa;
pub use certs::CertificateStore;
pub use crl::SignedCrl;
pub use material::Identity;
pub use pem::pem_to_der;
pub use role::{extract_role, extract_serial, extract_validity, ClusterRole, GOBLE_ROLE_OID};
pub use verifier::{ClusterClientVerifier, ClusterServerVerifier};
