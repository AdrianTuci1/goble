# Goble Identity & Worker Provisioning Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Make Goble identity a portable, password-protected wallet that each team owns. Team members join a cluster without entering IPs or passwords. Workers self-provision using a one-time invite token.

**Architecture:**
- A **cluster identity** is the root of trust (CA + cluster key). Only the creator/owner holds it, encrypted with a passphrase.
- A **device identity** is a portable wallet file containing: device certificate, device private key, CA certificate, and known peer list. It is encrypted with a user passphrase.
- A **worker** is a device with role `Worker` in the cluster. It stores its own device identity and cluster secrets (LLM keys, cloud creds) in its own encrypted vault.
- Members are added by **inviting their public key** (or sending a one-time token). The owner/admin signs a device certificate for them.
- Discovery: devices learn peer addresses from any reachable peer. A lightweight rendezvous endpoint helps with initial bootstrap.

**Tech Stack:** Rust (goble-core, goblin-worker, goble-desktop Tauri), TypeScript/React (goble-desktop UI), X.509/Ed25519 cluster PKI already present.

---

## Task 1: Encrypt cluster identity snapshot on disk

**Objective:** Ensure `cluster_identity` is never persisted as plain JSON; require a passphrase to create/import.

**Files:**
- Create: `crates/goble-core/src/encrypted_wallet.rs`
- Modify: `crates/goble-desktop/src-tauri/src/state.rs`
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs`
- Modify: `crates/goble-desktop/src/pages/SettingsPage.tsx`
- Modify: `crates/goble-desktop/src/tauri/api.ts`

**Step 1: Write failing test**

In `crates/goble-core/src/encrypted_wallet.rs`:

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncryptedWallet {
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

pub fn seal(plaintext: &[u8], passphrase: &[u8]) -> Result<EncryptedWallet> {
    // TODO
}

pub fn open(wallet: &EncryptedWallet, passphrase: &[u8]) -> Result<Vec<u8>> {
    // TODO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let data = b"cluster identity data";
        let wallet = seal(data, b"password123").unwrap();
        let opened = open(&wallet, b"password123").unwrap();
        assert_eq!(opened, data.as_slice());
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let data = b"cluster identity data";
        let wallet = seal(data, b"password123").unwrap();
        assert!(open(&wallet, b"wrong").is_err());
    }
}
```

**Step 2: Run test to verify failure**

```bash
cd /root/goble
cargo test -p goble-core encrypted_wallet
```

Expected: compile or runtime failure because `seal`/`open` are stubs.

**Step 3: Implement `seal`/`open` using existing crypto**

Reuse `crypto::encrypt_with_passphrase` / `decrypt_with_passphrase` (or implement AES-GCM + Argon2). Store `salt`, `nonce`, and `ciphertext`.

**Step 4: Run tests to verify pass**

```bash
cargo test -p goble-core encrypted_wallet
```

Expected: 2 tests passed.

**Step 5: Commit**

```bash
git add crates/goble-core/src/encrypted_wallet.rs crates/goble-core/src/lib.rs
git commit -m "feat(crypto): add EncryptedWallet seal/open primitives"
```

---

## Task 2: Replace plain cluster identity storage with encrypted wallet

**Objective:** Persist cluster identity as an encrypted wallet file on the desktop.

**Files:**
- Modify: `crates/goble-desktop/src-tauri/src/state.rs` (rename snapshot to wallet, add passphrase methods)
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs` (update commands)

**Step 1: Write failing test**

In `state.rs` tests:

```rust
#[test]
fn test_cluster_identity_encrypted_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let state = DesktopState::new(tmp.path().to_path_buf());
    state.create_cluster_encrypted("my-cluster", b"pass123").unwrap();
    let loaded = DesktopState::load(tmp.path().join("cluster.wallet"), b"pass123").unwrap();
    assert_eq!(loaded.get_cluster_identity().unwrap().cluster_name, "my-cluster");
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble-desktop-tauri state::tests::test_cluster_identity_encrypted_roundtrip
```

Expected: FAIL — function not found.

**Step 3: Implement encrypted wallet persistence**

- Change `cluster_identity` to `Option<EncryptedWallet>`.
- On `create_cluster_encrypted(name, passphrase)`: generate `ClusterIdentity`, serialize to plaintext, seal, write to `cluster.wallet`.
- On `load(wallet_path, passphrase)`: read file, open wallet, deserialize `ClusterIdentity`, store in memory.
- Add `unlock_cluster_identity(passphrase)` command.

**Step 4: Run test to verify pass**

```bash
cargo test -p goble-desktop-tauri state::tests
```

Expected: tests passed.

**Step 5: Commit**

```bash
git add crates/goble-desktop/src-tauri/src/state.rs
git commit -m "feat(desktop): store cluster identity as encrypted wallet"
```

---

## Task 3: Add device identity export/import (portable wallet)

**Objective:** A user can export their identity as a file and import it on another device to join the same cluster without re-entering cluster key.

**Files:**
- Create: `crates/goble-core/src/device_identity.rs`
- Modify: `crates/goble-core/src/lib.rs`
- Modify: `crates/goble-desktop/src-tauri/src/state.rs`
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs`
- Modify: `crates/goble-desktop/src/tauri/api.ts`
- Modify: `crates/goble-desktop/src/pages/SettingsPage.tsx`

**Step 1: Write failing test**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceIdentity {
    pub device_cert_pem: String,
    pub device_key_pem: String,
    pub ca_cert_pem: String,
    pub peer_addresses: Vec<String>,
}

pub fn export_device_identity(identity: &DeviceIdentity, passphrase: &[u8]) -> Result<Vec<u8>>;
pub fn import_device_identity(blob: &[u8], passphrase: &[u8]) -> Result<DeviceIdentity>;
```

Test in `device_identity.rs`.

**Step 2: Run test to verify failure**

```bash
cargo test -p goble-core device_identity
```

Expected: FAIL.

**Step 3: Implement using EncryptedWallet**

Serialize `DeviceIdentity` to JSON, seal with passphrase, return bytes (or base64 for transport).

**Step 4: Run test to verify pass**

```bash
cargo test -p goble-core device_identity
```

Expected: PASS.

**Step 5: Wire Tauri commands**

Add `export_identity(passphrase: String) -> Result<String, String>` and `import_identity(blob: String, passphrase: String) -> Result<(), String>`.

**Step 6: Add UI in SettingsPage Keys tab**

Replace placeholder with buttons: "Export identity", "Import identity", both requiring passphrase.

**Step 7: Commit**

```bash
git add crates/goble-core/src/device_identity.rs crates/goble-desktop/src-tauri/src/state.rs crates/goble-desktop/src-tauri/src/lib.rs crates/goble-desktop/src/tauri/api.ts crates/goble-desktop/src/pages/SettingsPage.tsx
git commit -m "feat(identity): export/import portable device identity wallet"
```

---

## Task 4: Add role-based member invitation flow

**Objective:** Owner/admin can add a member by inviting their public key; the new member receives a device certificate, never the cluster key.

**Files:**
- Modify: `crates/goble-core/src/identity.rs` (add `sign_device_csr` or similar)
- Modify: `crates/goble-desktop/src-tauri/src/state.rs`
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs`
- Modify: `crates/goble-desktop/src/pages/SettingsPage.tsx`
- Modify: `crates/goble-desktop/src/tauri/api.ts`

**Step 1: Write failing test**

```rust
#[test]
fn test_sign_device_public_key() {
    let cluster = ClusterIdentity::generate("c", "desktop", ClusterRole::Owner).unwrap();
    let device_key = KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
    let cert_pem = cluster.ca.sign_device_from_public_key(&device_key, "friend-device", ClusterRole::Viewer, 365).unwrap();
    let role = extract_role(&cert_pem).unwrap();
    assert_eq!(role, ClusterRole::Viewer);
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble-core identity::tests::test_sign_device_public_key
```

Expected: FAIL.

**Step 3: Implement `sign_device_from_public_key` on `ClusterCa`**

Build a CSR-less certificate from a public key and role. Add `public_key_only_sign` helper.

**Step 4: Run test to verify pass**

```bash
cargo test -p goble-core identity::tests::test_sign_device_public_key
```

Expected: PASS.

**Step 5: Add Tauri command `invite_member(public_key_pem: String, role: String, name: String) -> Result<String, String>`**

Returns a base64-encoded `DeviceIdentity` blob (certificate + CA cert + empty peers) that the new member imports.

**Step 6: Add UI in SettingsPage Members tab**

- Input: name, role dropdown, public key textarea.
- Button: "Generate invite identity".
- Display base64 blob + copy button.
- (Optional) QR code for the blob.

**Step 7: Commit**

```bash
git add crates/goble-core/src/identity.rs crates/goble-desktop/src-tauri/src/state.rs crates/goble-desktop/src-tauri/src/lib.rs crates/goble-desktop/src/tauri/api.ts crates/goble-desktop/src/pages/SettingsPage.tsx
git commit -m "feat(invite): role-based member invitation by public key"
```

---

## Task 5: Worker self-provisioning with one-time invite token

**Objective:** User does not need to enter SSH key / IP / worker URL. The desktop generates an invite token; a one-liner script on the VPS bootstraps the worker using that token.

**Files:**
- Create: `crates/goble-desktop/src-tauri/src/invite_token.rs`
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs`
- Create: `scripts/install-goblin-worker.sh`
- Modify: `crates/goblin-worker/src/main.rs`
- Modify: `crates/goblin-worker/src/pairing.rs`

**Step 1: Write failing test**

```rust
#[test]
fn test_invite_token_roundtrip() {
    let token = InviteToken::new(Worker, "ws://desktop:8787/ws").sign(&cluster_key).unwrap();
    let decoded = InviteToken::verify(&token, &ca_cert_pem).unwrap();
    assert_eq!(decoded.role, Worker);
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble-desktop-tauri invite_token
```

Expected: FAIL.

**Step 3: Implement invite token**

JWT-like signed token containing: `role=Worker`, `desktop_url`, `expires_at`, `one_time_nonce`. Signed with cluster CA key. Store used nonces in `state.rs` to prevent replay.

**Step 4: Run test to verify pass**

```bash
cargo test -p goble-desktop-tauri invite_token
```

Expected: PASS.

**Step 5: Create install script**

`scripts/install-goblin-worker.sh`:

```bash
#!/bin/bash
set -e
TOKEN="$1"
URL="${2:-https://github.com/AdrianTuci1/goble/releases/latest/download/goblin-linux-amd64}"
curl -fsSL "$URL" -o /usr/local/bin/goblin
chmod +x /usr/local/bin/goblin
/usr/local/bin/goblin --invite "$TOKEN" --daemon
```

**Step 6: Add `--invite` flag to goblin-worker**

In `main.rs`, parse `--invite`. Worker uses the token to connect back to the desktop URL, perform a handshake, and receive its device certificate + CA cert + initial peer list. Then save its own encrypted `device_identity` and `device_passphrase` (auto-generated or set via env).

**Step 7: Add Tauri command `generate_worker_invite() -> Result<String, String>`**

Returns one-liner command: `curl .../install-goblin-worker.sh | bash -s -- <token>`.

**Step 8: UI in SettingsPage Compute tab**

Replace placeholder with button: "Add worker". Generates invite, shows copy-paste one-liner + QR.

**Step 9: Commit**

```bash
git add crates/goble-desktop/src-tauri/src/invite_token.rs scripts/install-goblin-worker.sh crates/goblin-worker/src/main.rs crates/goblin-worker/src/pairing.rs crates/goble-desktop/src/pages/SettingsPage.tsx crates/goble-desktop/src/tauri/api.ts
git commit -m "feat(worker): self-provisioning with signed invite token"
```

---

## Task 6: Peer discovery and address sync

**Objective:** Devices do not need manual worker URLs after first bootstrap. Known peers exchange address lists.

**Files:**
- Modify: `crates/goblin-worker/src/main.rs`
- Modify: `crates/goblin-worker/src/websocket.rs`
- Modify: `crates/goble-core/src/protocol.rs`
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs` (rendezvous endpoint if needed)

**Step 1: Write failing test**

```rust
#[test]
fn test_peer_list_merge() {
    let mut peers = PeerList::new();
    peers.merge(vec!["ws://a:8787/ws".to_string(), "ws://b:8787/ws".to_string()]);
    assert_eq!(peers.addresses.len(), 2);
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble-core protocol::tests::test_peer_list_merge
```

Expected: FAIL.

**Step 3: Implement peer list type**

In `goble-core`, add `PeerList` and messages:

```rust
DesktopMessage::PeerListRequest,
WorkerMessage::PeerList { addresses: Vec<String> },
```

**Step 4: Implement discovery in worker**

On handshake, worker sends its own address and receives peer list. It stores peers in `AppState`. On next startup, it tries all known peers.

**Step 5: Implement in desktop Tauri (optional)**

Desktop can serve a small HTTP `/peers` endpoint or answer via WebSocket.

**Step 6: Commit**

```bash
git add crates/goble-core/src/protocol.rs crates/goblin-worker/src/main.rs crates/goblin-worker/src/websocket.rs
git commit -m "feat(discovery): peer address list sync between devices"
```

---

## Task 7: Enable mTLS for desktop ↔ worker WebSocket

**Objective:** Replace plain WebSocket with certificate-based mTLS using the cluster PKI.

**Files:**
- Modify: `crates/goblin-worker/src/main.rs` (load device identity, configure rustls)
- Modify: `crates/goblin-worker/src/websocket.rs` (extract client role from cert)
- Modify: `crates/goble-desktop/src-tauri/src/lib.rs` (connect worker using mTLS)
- Modify: `crates/goble-desktop/src-tauri/tauri.conf.json` (permissions if any)
- Modify: `crates/goblin-worker/src/pairing.rs` (remove pairing hash if mTLS replaces it)

**Step 1: Write failing test**

Use existing `PairingBundle` or build a small integration test in `tests/worker_e2e.rs`.

**Step 2: Run test to verify failure**

```bash
cargo test -p goblin-worker worker_e2e
```

Expected: FAIL if mTLS not wired.

**Step 3: Configure worker rustls with cluster client verifier**

Use `ClusterClientVerifier` to accept only `Owner`/`Admin`/`Operator`/`Worker` client certs. Load device identity from encrypted wallet at startup.

**Step 4: Configure desktop client**

Use `ClusterServerVerifier` to verify worker cert has role `Worker`. Desktop loads its own device identity.

**Step 5: Remove or keep pairing hash optional**

If mTLS validates identity, pairing hash becomes optional/secondary. Keep for backwards compatibility but make it optional behind a flag.

**Step 6: Commit**

```bash
git add crates/goblin-worker/src/main.rs crates/goblin-worker/src/websocket.rs crates/goble-desktop/src-tauri/src/lib.rs
git commit -m "feat(tls): mTLS between desktop and worker using cluster identity"
```

---

## Task 8: Full integration test and docs

**Objective:** Prove the end-to-end flow: owner creates cluster, invites friend, friend imports identity, worker self-provisions, desktop connects to worker via mTLS.

**Files:**
- Create: `tests/identity_and_worker_e2e.rs`
- Modify: `docs/architecture.md` (or create `docs/identity.md`)

**Step 1: Write integration test**

```rust
#[tokio::test]
async fn test_cluster_invite_worker_flow() {
    // 1. owner creates encrypted cluster
    // 2. owner exports friend device identity
    // 3. friend imports identity
    // 4. owner generates worker invite token
    // 5. worker starts with token, receives device cert
    // 6. desktop connects to worker via mTLS and sends Ping
}
```

**Step 2: Run test to verify failure**

```bash
cargo test -p goble identity_and_worker_e2e
```

Expected: FAIL initially, then PASS after fixing issues.

**Step 3: Fix issues until pass**

**Step 4: Write docs**

`docs/identity.md`:
- How to create a cluster.
- How to export/import identity.
- How to invite a member.
- How to add a worker.
- Security model: what is encrypted, what is signed, what never leaves the device.

**Step 5: Commit**

```bash
git add tests/identity_and_worker_e2e.rs docs/identity.md
git commit -m "test(docs): identity and worker provisioning end-to-end"
```

---

## Risks & Open Questions

1. **Key storage passphrase UX:** User must enter passphrase at app startup. Do we cache in OS keyring to avoid typing every time? (Yes, but only for device key, not cluster key for non-owners.)
2. **Rendezvous server:** If both desktop and worker are behind NAT, peer discovery may need a public rendezvous. Is a public rendezvous acceptable for an open-source project? (Yes, but it should only relay encrypted mTLS handshakes, never see data.)
3. **Worker auto-generated passphrase:** Where does the worker store its passphrase? Options: derived from token, stored in OS keyring, or user must enter on first start. Suggest: derive from invite token + local salt, then rotate on first manual unlock.
4. **CRL distribution:** When a device is revoked, how do other devices learn the CRL? Suggest: gossip over WebSocket, signed by CA, versioned monotonic.
