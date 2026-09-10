//! Optional OS isolation backends.
//!
//! `seatbelt` is a *real* macOS backend: it is gated behind the `unix-seatbelt`
//! feature *and* `target_os = "macos"`, and it opts in to `unsafe` (the
//! workspace denies it by default) to compile the profile into an `.sb` policy
//! and apply it with `sandbox_apply`. It is selected for hardened profiles on
//! macOS (see [`crate::sandbox_for`]).
//!
//! The Linux kernel-call backends (Landlock, seccomp) are gated behind a
//! `unix-*` cargo feature *and* `target_os = "linux"`. Because the workspace
//! denies `unsafe`, these declare the seam and decline to harden a hardened
//! profile rather than compile syscalls; a build that opts in to `unsafe` wires
//! the real kernel call there. On the common build they are never compiled.
//!
//! `bwrap` is the exception on Linux: it hardens a hardened profile by spawning
//! the `bwrap` executable, which needs no `unsafe`. It is therefore always
//! compiled and is selected for hardened profiles on Linux.

#[cfg(all(feature = "unix-seatbelt", target_os = "macos"))]
pub mod seatbelt;

#[cfg(all(feature = "unix-landlock", target_os = "linux"))]
pub mod landlock;

#[cfg(all(feature = "unix-seccomp", target_os = "linux"))]
pub mod seccomp;

pub mod bwrap;
