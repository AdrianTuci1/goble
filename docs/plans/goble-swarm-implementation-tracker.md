# Goble Swarm Implementation Tracker

**Branch:** `feature/agent-guide-ui`  
**Base commit after Phase 1:** `1a5e497`

## Legend

- `[ ]` pending
- `[>]` in progress
- `[x]` done

---

## Phase 1 — Snapshot engine `[x]`

- **Objective:** Add encrypted object-storage snapshot format that workers can upload/restore for disaster recovery.
- **Files touched:**
  - `crates/goble-core/src/snapshot.rs` (new)
  - `crates/goble-core/src/store.rs`
  - `crates/goble-core/src/protocol.rs`
  - `crates/goblin-worker/src/snapshot_runner.rs` (new)
  - `crates/goblin-worker/src/state.rs`
  - `crates/goblin-worker/src/main.rs`
  - `crates/goblin-worker/src/websocket.rs`
  - `crates/goble-cli/src/lib.rs`
- **Tests added:**
  - `goble-core/src/store.rs::test_snapshot_export_import_roundtrip`
  - `goblin-worker/src/snapshot_runner.rs::test_restore_empty_store`
- **Verification command:**
  ```bash
  cargo test -p goble-core -p goblin-worker -p goble-cli -- --skip test_multi_worker_round_robin_dispatch --skip test_agent_runtime_isolation_and_secret_passthrough --skip test_worker_health_and_websocket_run_agent
  ```
- **Commit:** `1a5e497`

---

## Phase 2 — Identity wallet as part of the snapshot `[x]`

- **Objective:** Move the raw cluster key into an encrypted `IdentityWallet` payload and include that wallet in the snapshot so a new device can restore identity without a central account server.
- **Files touched:**
  - `crates/goble-core/src/encrypted_wallet.rs`
  - `crates/goble-core/src/store.rs`
  - `crates/goble-cli/src/lib.rs`
  - `crates/goble-desktop/src-tauri/src/state.rs`
- **Tests added:**
  - `goble-core/src/store.rs::test_identity_wallet_roundtrip_in_snapshot`
- **Implementation:**
  1. Defined `IdentityWallet { version, cluster_key_base64, cluster_name, ca_cert_pem, ca_key_pem, revoked_serials, devices, workers }`.
  2. Added `IdentityWallet::seal`/`open` and `From<&ClusterIdentity>`/`to_cluster_identity` helpers.
  3. Changed `Store::{set_cluster_wallet, get_cluster_wallet}` to use dedicated `cluster_wallet` setting key.
  4. Pruned `SNAPSHOT_TABLES` to match real tables in `Store::init`.
  5. Added CLI `goble identity {create,export,restore}`.
  6. Updated desktop `set_cluster_identity`/`unlock_cluster_identity` to use `IdentityWallet`.
- **Verification command:**
  ```bash
  cargo test --workspace -- --skip test_multi_worker_round_robin_dispatch --skip test_agent_runtime_isolation_and_secret_passthrough --skip test_worker_health_and_websocket_run_agent
  ```
- **Commit step:** `feat: identity wallet embedded in encrypted snapshot`

---

## Phase 3 — Certificate-based worker provisioning `[ ]`

- **Objective:** Replace the pairing-code hash bootstrap with a certificate bundle. The desktop signs a worker certificate with the cluster CA and the worker starts with `--bundle worker-bundle.json`.
- **Files to touch:**
  - `crates/goble-core/src/provision.rs`
  - `crates/goble-core/src/identity.rs`
  - `crates/goble-core/src/tls.rs`
  - `crates/goblin-worker/src/main.rs`
  - `crates/goblin-worker/src/pairing.rs`
  - `crates/goble-cli/src/lib.rs`
- **Failing test to drive implementation:**
  - `goble-core/src/provision.rs::tests::test_provision_bundle_contains_worker_cert`
- **Implementation:**
  1. Add `WorkerBundle { worker_id, cert_pem, key_pem, ca_cert_pem, cluster_name }`.
  2. `ClusterCa::sign_worker(worker_id)` returns the bundle.
  3. `SshTransport` copies binary + bundle + starts worker with `--bundle`.
  4. Worker reads bundle, builds server mTLS config, removes hash-based `pair_hash` check.
  5. Desktop connects to worker over mTLS using its device cert.
- **Verification command:**
  ```bash
  cargo test -p goble-core provision identity tls
  cargo test -p goble-cli --test e2e_worker --test tls_and_setup
  cargo test -p goblin-worker pairing
  ```
- **Commit step:** `feat: certificate-based worker provisioning replaces pairing hash`

---

## Phase 4 — Device sync via snapshot `[x]`

- **Objective:** A new device restores identity and worker metadata from the snapshot store; live runtime data stays on the workers and is queried directly.
- **Files touched:**
  - `crates/goble-core/src/device_transfer.rs` (new)
  - `crates/goble-core/src/lib.rs`
  - `crates/goble-cli/src/lib.rs`
  - `crates/goble-cli/tests/tls_and_setup.rs`
- **Failing test to drive implementation:**
  - `goble-core::device_transfer::tests::test_device_restore_from_snapshot`
- **Implementation:**
  1. `DeviceTransfer::restore_from_snapshot` downloads latest snapshot, decrypts it with cluster key, extracts `IdentityWallet`, and decrypts it with passphrase.
  2. Generates a new device certificate via `ClusterIdentity::from_key`, adds it to wallet.
  3. Returns `(IdentityWallet, Identity)` so the CLI can persist the updated wallet locally.
  4. CLI adds `goble device restore --from-snapshot <dir> --cluster-key <key> --passphrase <pass>`.
  5. No local data migration: restored wallet contains worker list/URLs; the device queries live workers for runtime state.
- **Verification command:**
  ```bash
  cargo test -p goble-core device_transfer
  cargo test -p goble-cli --test tls_and_setup --test e2e_worker
  cargo test --workspace --lib
  ```
- **Commit step:** `feat: restore identity wallet on a new device from snapshot`

---

## Phase 5 — Kubernetes cluster mode with snapshot tier `[x]`

- **Objective:** Run workers in Kubernetes. Each pod has local state; snapshot tier is cluster-level. No live runtime migration. Sticky sessions handled at the UI/balancer layer.
- **Files touched:**
  - `deploy/goblin/Dockerfile`
  - `deploy/goblin/charts/goblin-cluster/` (Helm chart)
  - `crates/goblin-worker/src/main.rs` (`--mode=cluster`, env-based snapshot config)
  - `crates/goblin-worker/src/state.rs` (PVC-aware restore guard)
  - `crates/goblin-worker/src/scheduler.rs` (leader election integration)
  - `crates/goblin-worker/src/leader.rs` (Kubernetes lease client)
  - `crates/goblin-worker/src/snapshot_runner.rs` (cluster-mode restore guard)
  - `crates/goblin-worker/Cargo.toml` (wiremock dev dependency)
  - `crates/goble-desktop/src-tauri/src/state.rs` (helm command generator)
  - `crates/goble-desktop/src-tauri/src/lib.rs` (`cluster_helm_install` Tauri command)
  - `crates/goble-desktop/src-tauri/Cargo.toml` (base64 dependency)
  - `crates/goble-desktop/src/tauri/api.ts` (cluster install API)
  - `crates/goble-desktop/src/components/ClusterInstallCard.tsx` (new)
  - `crates/goble-desktop/src/pages/SettingsPage.tsx` (cluster install card)
  - `crates/goble-cli/src/lib.rs` (Helm command generator)
  - `crates/goble-cli/tests/tls_and_setup.rs` (helm-install parsing test)
- **Tests added:**
  - `goblin-worker/src/snapshot_runner.rs::test_cluster_mode_skips_restore_when_db_exists`
  - `goblin-worker/src/snapshot_runner.rs::test_cluster_mode_restores_when_db_missing`
  - `goblin-worker/src/scheduler.rs::test_scheduler_loop_skips_triggers_when_not_leader`
  - `goblin-worker/src/leader.rs::test_kube_leader_elector_acquires_new_lease`
  - `goblin-worker/src/leader.rs::test_kube_leader_elector_yields_to_existing_holder`
  - `goble-cli/tests/tls_and_setup.rs::test_cluster_helm_install_subcommand_parsing`
- **Verification command:**
  ```bash
  cargo test -p goblin-worker --lib snapshot_runner scheduler leader
  cargo test -p goble-cli --test tls_and_setup
  ```

---

## Phase 6 — Worker groups and runtime routing `[ ]`

- **Objective:** Tag workers into groups (e.g. `gpu`, `prod`) and let the desktop route agents to the best worker using `WorkerPool`.
- **Files to touch:**
  - `crates/goble-core/src/worker_pool.rs`
  - `crates/goble-core/src/worker.rs`
  - `crates/goblin-worker/src/state.rs`
  - `crates/goble-cli/src/lib.rs` (`goble worker tag`)
  - `crates/goble-desktop/src/components/ComposerRuntimeSelector.tsx` (new)
- **Failing test to drive implementation:**
  - `goble-core::worker_pool::tests::test_tagged_group_selection`
- **Implementation:**
  1. Add `tags: Vec<String>` to `Worker`.
  2. `WorkerPool::select` accepts optional tag filter.
  3. Desktop UI shows runtime selector: local / group / specific worker.
  4. CLI `goble worker tag <worker-id> <tag>` updates worker metadata.
- **Verification command:**
  ```bash
  cargo test -p goble-core worker_pool worker
  cargo test -p goble-cli --test tls_and_setup
  ```
- **Commit step:** `feat: worker groups and runtime routing`

---

## Phase 7 — Compliance & security hardening `[ ]`

- **Objective:** Close security gaps: encrypted device store, no empty vault passphrase, audit log, key rotation.
- **Files to touch:**
  - `crates/goble-core/src/store.rs` (encryption at rest)
  - `crates/goble-core/src/vault.rs`
  - `crates/goble-core/src/encrypted_wallet.rs`
  - `crates/goble-core/src/audit.rs` (new)
  - `crates/goble-cli/src/lib.rs` (`goble identity rotate-worker-certs`)
  - `crates/goble-desktop/src/components/IdentitySettings.tsx`
- **Failing test to drive implementation:**
  - `goble-core::vault::tests::test_empty_passphrase_rejected`
- **Implementation:**
  1. Enforce non-empty vault passphrase.
  2. Encrypt device SQLite store with key derived from device key + passphrase.
  3. Add `AuditLog` table; log every signed command.
  4. Add `rotate-worker-certs` command that re-issues all worker certificates and pushes CRL update.
  5. UI warns until wallet is exported.
- **Verification command:**
  ```bash
  cargo test -p goble-core vault encrypted_wallet audit identity
  cargo test -p goble-cli
  cargo test -p goblin-worker
  ```
- **Commit step:** `feat: compliance hardening — encrypted store, audit log, key rotation`

---

## Cross-cutting concerns

### Kubernetes / autoscale / sticky sessions

- **No live runtime migration.** A running agent, its MCP child processes, and LLM streams are bound to one pod. Autoscale must only scale new work, never migrate running agents.
- **PVC per pod.** Use a `StatefulSet` so each pod keeps a stable name and local `worker.db` across restarts.
- **Snapshot tier is cluster-level.** All pods share the same bucket, prefix, and credentials.
- **Sticky sessions.** The desktop must send a running agent's traffic to the same pod. Options:
  1. Expose each pod behind its own stable DNS / LoadBalancer.
  2. Use Kubernetes Service `sessionAffinity: ClientIP` (works only for same source IP and short-lived flows).
  3. Have the desktop track the pod assigned to each agent and route directly to it.
- **Leader election.** Only one pod should run cron-style `Scheduler` loops. Use Kubernetes `coordination.k8s.io` leases.
