//! Proves the release signer and the client verifier agree.
//!
//! A signature scheme with two independent implementations is one typo away from
//! rejecting every real release, and the failure appears at release time rather
//! than in CI. So the packaging script is executed here, on a throwaway
//! manifest, and its output is verified with the same code the app uses.
//!
//! Skipped when `python3` is not available: the signer needs it.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use goble_update::manifest::Channel;
use ring::signature::KeyPair as _;

/// A throwaway key. Never used to sign anything real.
const SEED: [u8; 32] = [7_u8; 32];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives in <repo>/crates/goble-update")
        .to_path_buf()
}

fn signer() -> PathBuf {
    repo_root().join("packaging/sign-manifest.sh")
}

fn have_python() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn write_manifest(path: &Path) {
    let manifest = serde_json::json!({
        "schema": 1,
        "channels": {
            "stable": {
                "version": "0.2.0",
                "released_at": "2026-09-01T10:00:00Z",
                "artifacts": [
                    {
                        "os": "macos",
                        "arch": "aarch64",
                        "url": "https://releases.goble.dev/stable/0.2.0/Goble-0.2.0-arm64.dmg",
                        "sha256": "a".repeat(64),
                        "size": 12_345_678_u64,
                    },
                    {
                        "os": "linux",
                        "arch": "x86_64",
                        "url": "https://releases.goble.dev/stable/0.2.0/Goble-0.2.0-x86_64.AppImage",
                        "sha256": "b".repeat(64),
                        "size": 9_876_543_u64,
                    }
                ]
            }
        }
    });
    std::fs::write(path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
}

fn expected_public_key() -> String {
    let key_pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&SEED).unwrap();
    hex::encode(key_pair.public_key().as_ref())
}

fn sign(manifest: &Path, with_key: bool) -> std::process::Output {
    let mut command = Command::new(signer());
    command.arg("--manifest").arg(manifest).arg("--inplace");
    if with_key {
        command.env("GOBLE_UPDATE_SIGNING_KEY", hex::encode(SEED));
    } else {
        command.env_remove("GOBLE_UPDATE_SIGNING_KEY");
    }
    command.output().expect("the signer script must be runnable")
}

#[test]
fn the_packaging_signer_and_the_client_verifier_agree() {
    if !have_python() {
        eprintln!("skipping: python3 is not installed, so the signer cannot run");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("channel_versions.json");
    write_manifest(&manifest_path);

    let output = sign(&manifest_path, true);
    assert!(
        output.status.success(),
        "signing failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = goble_update::ChannelManifest::parse(&text).expect("a parseable manifest");
    let release = manifest.release(Channel::Stable).expect("a valid stable release");
    release
        .verify_signature(&expected_public_key())
        .expect("the signature the packaging script wrote must verify");

    // And the same check has to reject a release edited after signing.
    let tampered = text.replace(&"b".repeat(64), &"c".repeat(64));
    assert_ne!(tampered, text, "the fixture must contain the digest being replaced");
    let manifest = goble_update::ChannelManifest::parse(&tampered).unwrap();
    let release = manifest.release(Channel::Stable).unwrap();
    assert!(
        release.verify_signature(&expected_public_key()).is_err(),
        "a tampered release must not verify"
    );
}

#[test]
fn the_signer_refuses_to_invent_a_signature_without_a_key() {
    if !have_python() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("channel_versions.json");
    write_manifest(&manifest_path);

    let output = sign(&manifest_path, false);
    assert!(!output.status.success(), "signing without a key must fail");

    let text = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(
        !text.contains("\"signature\""),
        "a failed signing run must not leave a signature behind: {text}"
    );
}
