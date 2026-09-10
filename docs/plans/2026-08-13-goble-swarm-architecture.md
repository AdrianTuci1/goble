# Goble Swarm — Complete Architecture Plan

## 1. Objective

Replace the current 1:1 desktop-to-worker model with a **swarm architecture** where:

- One human user = one identity = one `ClusterKey`.
- The same identity is used on desktop, laptop, phone, web, or any future UI device.
- The user owns multiple **workers** (local process, remote VPS, Kubernetes cluster) without a dedicated central database.
- Configuration (add worker, sync identity, create worker group, install cluster) is driven from chat via custom composer cards.
- Workers never hold cluster secrets in plain text; runtime metadata is ephemeral or pushed by the user's devices.
- Workers can optionally persist an encrypted snapshot to S3/R2/MinIO for disaster recovery; without it they are purely ephemeral.

This plan is **not an MVP**. It describes the target architecture and the concrete implementation path.

## 2. Core Principles

1. **Single source of truth: the live worker + optional encrypted snapshot backup.**
   - The desktop UI queries workers directly for agents, executions, and schedules. It does not download the snapshot during normal operation.
   - The snapshot is an encrypted disaster-recovery bundle stored on S3/R2/MinIO/Backblaze B2.
   - Workers download the snapshot only when they have lost local state (new pod, corrupted data, explicit restore).
   - Devices keep a local cache of what they have seen from workers; if lost, they restore from the snapshot store.
   - No Postgres, no etcd, no shared database between devices or workers.
2. **Workers are drones, not peers.**
   - Workers receive signed commands from devices and store runtime state locally.
   - Workers do not talk to each other directly.
   - Worker-to-device trust is established by mTLS certificates signed by the cluster CA.
3. **Cluster mode = identity federation + optional disaster-recovery snapshot, not shared database.**
   - A Kubernetes cluster is one or more worker pods sharing the same `ClusterKey` and the same cluster snapshot store.
   - Snapshot backup is configured once per cluster, not per pod. All pods share the same bucket/path.
   - Each pod runs live state locally; persistence is local PVC + cluster-level snapshot for recovery.
   - If a pod dies and loses its PVC, the cluster recovers from the latest snapshot automatically when the snapshot tier is enabled.
4. **Disaster recovery is opt-in and user-controlled.**
   - By default a worker runs live state only.
   - The user can enable snapshot backup, set a frequency, and choose retention.
5. **Configuration is conversational.**
   - Every complex action (add worker, install K8s, invite device, tag worker, enable snapshot backup) is surfaced as a composer card sequence.
   - The LLM/harness does not execute infra commands directly; it generates the exact command or manifest for the user to run or approve.

## 3. Identity Model — One Key, Many Devices

### 3.1 ClusterKey as root

The existing `ClusterKey` in `crates/goble-core/src/cluster_key.rs` is already the correct primitive:

- 32-byte seed deterministically derives the Ed25519 CA and symmetric backup/vault keys.
- The seed is the only thing the user must back up.

### 3.2 Encrypted wallet is the portable identity

`crates/goble-core/src/encrypted_wallet.rs` already provides `EncryptedWallet::seal/open`. Expand the payload to include:

```rust
pub struct IdentityWallet {
    pub version: u32,
    pub cluster_key_base64: String,        // the single user seed
    pub cluster_name: String,
    pub ca_cert_pem: String,
    pub ca_key_pem: String,                // owner only
    pub revoked_serials: Vec<String>,
    pub devices: Vec<DeviceEntry>,       // certs for all known devices
    pub workers: Vec<WorkerEntry>,         // known worker IDs + URLs
}
```

- Each device stores its own device cert in the wallet.
- The wallet is encrypted with the **vault passphrase** plus a **device-local key** (OS keyring / iOS Keychain / Android Keystore / DPAPI).
- Export/import of the wallet is the sync mechanism.

### 3.3 Device enrollment flow

1. User on device A chooses "Add another device".
2. Device A generates a one-time transfer code (e.g., a 12-word phrase or a QR code).
3. Device B scans/enters the code.
4. Device A sends the encrypted wallet blob over a temporary end-to-end encrypted channel (QR contains a public key + short-range rendezvous URL).
5. Device B decrypts, generates its own device certificate, and stores the updated wallet.
6. Device A marks device B as active in its wallet.

No central account server. The user owns the sync channel.

### 3.4 Roles

Use existing `ClusterRole` from `crates/goble-core/src/identity.rs`:

- `Owner` — the human user; can sign worker certs and device certs.
- `Admin` — trusted device role; can issue device certs? No. Only Owner can issue device certs. Admin can operate workers.
- `Operator` — can run agents, view traces.
- `Viewer` — read-only.
- `Worker` — worker pods/VPS.

## 4. Worker Model — Local, Remote, Cluster

### 4.1 Three concrete deployment modes

| Mode | Form | Use case | How user adds it |
|---|---|---|---|
| **Local** | Same process as desktop UI or a child process | Quick tests, offline work | Automatic; toggle in UI |
| **Remote** | Single binary on a VPS/bare metal, systemd service | Personal production worker | SSH provisioning + pairing certificate |
| **Cluster** | K8s StatefulSet / Helm chart | Team/org scale, GPU nodes | Helm install command generated by chat |

### 4.2 Local worker

- The desktop app can spawn a worker in-process or as a hidden child process.
- Uses the same `goblin` binary embedded in the app bundle.
- No network exposure.
- Workspace root in app data directory.

### 4.3 Remote worker

Keep the current provisioning path from `crates/goble-core/src/provision.rs` but replace pairing-code hash with certificate-based bootstrap:

1. User provides host + SSH credentials via composer card.
2. Desktop generates a `Worker` role certificate signed by cluster CA.
3. Desktop scps the `goblin` binary + a `worker-bundle.json` containing:
   - worker_id
   - worker certificate + key
   - CA certificate
   - cluster name
   - optional: vault key for secrets (sent separately, encrypted)
4. Remote worker starts with `--bundle worker-bundle.json`.
5. Worker connects back to desktop via mTLS WebSocket, or desktop connects to worker HTTPS+mTLS.

### 4.4 Cluster worker

In Kubernetes, the **cluster snapshot tier** is configured once per cluster. Every pod reads and writes the same snapshot object.

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: goblin-cluster
  namespace: goblin
spec:
  replicas: 2
  template:
    spec:
      serviceAccountName: goblin
      containers:
        - name: goblin
          image: ghcr.io/goble/goblin:latest
          args:
            - --mode=cluster
            - --cluster-name=$(CLUSTER_NAME)
            - --snapshot-enabled=$(SNAPSHOT_ENABLED)
            - --snapshot-interval=$(SNAPSHOT_INTERVAL)
            - --snapshot-bucket=$(SNAPSHOT_BUCKET)
          env:
            - name: CLUSTER_NAME
              value: my-cluster
            - name: SNAPSHOT_ENABLED
              value: "false"      # set "true" once the user enables disaster recovery
            - name: SNAPSHOT_INTERVAL
              value: "3600"       # seconds
            - name: SNAPSHOT_BUCKET
              value: s3://my-goble-snapshot
          envFrom:
            - secretRef:
                name: goblin-snapshot-credentials
          volumeMounts:
            - name: data
              mountPath: /var/goblin
      volumes:
        - name: data
          emptyDir: {}             # live state is ephemeral unless a PVC class is supplied
```

Key points:

- Snapshot backup is a cluster-level setting, not per pod.
- All pods in the Deployment share the same bucket, prefix, and credentials.
- A pod that starts finds the latest snapshot and downloads it; live state is reconstructed from there.
- Snapshot frequency and retention are configured by the user once for the cluster.
- Local PVC is optional; by default pods use `emptyDir` for live state.
- Long-running executions are interrupted when a pod restarts; snapshot only restores configuration, schedules, and secrets, not live workspace streams.

### 4.5 Why no shared DB and no autoscale

Because:

- A running agent's workspace, MCP child processes, and LLM streams are **bound to a pod**.
- Moving a live agent between pods requires migration of runtime state that we do not support.
- Without central DB, the swarm remains operationally simple: each worker is self-contained, devices route work to the right worker, and a failed worker is replaced by re-running agents.

## 5. Storage Model — No Dedicated Database

### 5.1 Device storage

Each device has a local SQLite cache (`goble_store.sqlite`) for fast queries of:

- agents, teams, workflows, executions, threads
- chat history
- worker connection metadata
- snapshot metadata (last synced version, timestamp, provider)

This store is a **cache** of the user's snapshot data. If a device is lost, it recovers from the snapshot store using the `ClusterKey` and passphrase.

### 5.2 Worker storage

A worker keeps:

- `worker.db` (SQLite) — live agents, teams, workflows, schedules, secrets.
- `vault.json` — encrypted secrets with the cluster vault key.
- `workspaces/<id>/` — live runtime files.
- Local execution history and traces.

When the cluster snapshot tier is enabled, the worker periodically uploads an encrypted snapshot of its configuration, schedules, secrets, and finalized traces to object storage. If the worker is rebuilt, rescheduled, or loses its local data, it downloads the latest snapshot and reconstructs its state. The UI never downloads the snapshot during normal operation; it only queries the live worker.

### 5.3 Snapshot contents

The encrypted snapshot contains:

1. Identity wallet (cluster key, CA, device certs, revoked serials).
2. Agent specs and schedules.
3. Workflows and teams.
4. MCP server definitions and MCP accounts.
5. Encrypted vault secrets.
6. Execution history (finalized traces).
7. Chat history and threads.
8. Worker/group metadata.

### 5.4 Snapshot lifecycle

| Event | Action |
|---|---|
| User enables snapshot | Worker uploads initial snapshot to the cluster bucket. |
| Worker starts (cluster mode, snapshot enabled, no local data) | Downloads latest snapshot and reconstructs state. |
| Worker starts (cluster mode, snapshot enabled, local data intact) | Continues with local data; uploads on next interval. |
| Configuration change (agent added, secret changed, etc.) | Worker queues incremental upload; may wait for next interval. |
| Snapshot interval elapses | Worker uploads full snapshot if it has local changes. |
| User disables snapshot | No more uploads; live state remains local only. |
| Device added | Device connects to an existing worker or restores from snapshot store. |
| Device lost | User restores device from snapshot store using bucket credentials. |

### 5.5 Persistence of executions

- The device that triggered an execution owns the finalized trace.
- Worker emits trace events over mTLS WebSocket; device persists them in its cache and pushes to the snapshot store.
- If a worker dies, the user sees the partial trace on the device and can retry.
- Live workspace files are not part of the snapshot; they are ephemeral.

## 6. Network & Trust Model

### 6.1 mTLS everywhere

Use existing `ClusterIdentity` / `ClusterClientVerifier` from `crates/goble-core/src/identity.rs`:

- Every device and every worker has a certificate signed by the same cluster CA.
- WebSocket connections are mTLS.
- Workers reject commands from unknown CAs.

### 6.2 Peer discovery

Devices learn about workers from the wallet `workers` list. Workers do not need to discover each other.

For initial setup when a device does not know a worker's address:
- If worker is local/remote, user enters URL.
- If worker is in Kubernetes, the Helm chart creates a Service/LoadBalancer; user enters the exposed endpoint.
- Optional: a lightweight rendezvous relay (can be self-hosted) helps devices find workers without static IPs. The relay only routes encrypted traffic; it does not hold keys.

### 6.3 Revocation

Use existing `CertificateStore` + `SignedCrl` from `crates/goble-core/src/identity.rs`:

- If a device is lost, the owner revokes its serial in the wallet and distributes the updated CRL to all workers.
- Workers cache the CRL and reject revoked certs.

## 7. Chat-Driven Configuration via Composer Cards

### 7.1 Design rule

The assistant does **not** run infrastructure commands on the user's machine. It produces:

- A composer card with the exact command/manifest.
- A "copy" or "run locally" action.
- Optional: a one-click Tauri command if the user has already granted local execution permission.

### 7.2 Composer card flows

#### A. Add a remote worker

User: *"add a worker on my VPS at 203.0.113.10"*

Card sequence:
1. Ask for SSH key or password (stored in device vault, not sent to LLM).
2. Generate worker certificate.
3. Show generated `goblin` install command + `scp` bundle command.
4. After user confirms, run provisioning via existing `SshTransport` in `crates/goble-core/src/provision.rs`.
5. Card result: worker appears in the worker list.

#### B. Install a Kubernetes cluster

User: *"install goble cluster in kubernetes"*

Card sequence:
1. Ask cloud provider / kubeconfig context.
2. Ask if disaster recovery (snapshot to S3/R2/MinIO) is enabled for the cluster. If yes, collect bucket + credentials once. This setting applies to the whole cluster, not to individual pods.
3. Generate Helm install command:
   ```bash
   helm repo add goble https://goble.sh/charts
   helm install goblin goble/goblin-cluster \
     --namespace goblin --create-namespace \
     --set clusterName=$(CLUSTER_NAME) \
     --set snapshot.enabled=true \
     --set snapshot.provider=r2 \
     --set snapshot.endpoint=https://... \
     --set snapshot.bucket=my-goble-snapshot \
     --set snapshot.accessKeyId=... \
     --set snapshot.secretAccessKey=... \
     --set snapshot.intervalSeconds=3600
   ```
4. User runs command in their terminal.
5. Card asks for exposed worker URL.
6. Desktop connects to the cluster worker via mTLS. The worker already has the cluster snapshot credentials and will upload to the bucket on schedule. The UI never downloads the snapshot unless the user explicitly chooses restore.

#### C. Sync identity to phone

User: *"add my phone"*

Card sequence:
1. Phone is added as a new device. It can join by:
   - connecting to an existing live worker and downloading the current state, or
   - restoring from the snapshot store if no live worker is reachable.
2. The desktop provides the cluster passphrase + snapshot bucket credentials (or a one-time transfer phrase if the snapshot tier is not enabled).
3. Phone downloads and decrypts the snapshot if needed, generates its own device certificate, and connects to workers.
4. Phone is listed under "Devices".

#### D. Create a worker group / tag

User: *"create a gpu group with worker-2"*

Card sequence:
1. Show available workers in a multi-select card.
2. Ask group name.
3. Persist tag in device cache and, if snapshot tier is enabled, push to worker for snapshot inclusion.
4. When running an agent, user can select the group; `WorkerPool` selects a member.

#### E. Enable disaster recovery for a cluster

User: *"enable backup for my kubernetes cluster"*

Card sequence:
1. Ask which cluster (if multiple).
2. Ask provider (R2/S3/MinIO/B2) and bucket credentials once. The setting applies to the whole cluster.
3. Ask snapshot interval (e.g., 1 hour, 6 hours, 24 hours) and retention count.
4. Generate a patch/Helm upgrade command that sets the snapshot env vars on the cluster.
5. User runs the command; from then on every pod in the cluster uploads to the same bucket.
6. UI shows last snapshot timestamp and size per cluster.

## 8. UI Changes

### 8.1 Settings → Identity

Replace "Cluster Key" raw input with:

- Identity card: cluster name, fingerprint, number of devices, number of workers.
- "Export wallet" — encrypted backup file.
- "Add device" — QR / transfer phrase.
- "Revoke device" — list of devices with revoke action.
- "Restore from backup" — import wallet file + passphrase.

### 8.2 Settings → Workers

Three tabs:

- **Local** — toggle local worker.
- **Remote** — list of provisioned VPS workers; add/remove.
- **Cluster** — list of K8s cluster workers; add via Helm command.

Each worker shows:
- name, role tags (cpu/gpu/prod), status, last seen, active traces.
- actions: reconnect, revoke, view logs.

### 8.3 Composer runtime selector

In composer header:

```
Run on: [ Local ▼ ]  [ GPU Group ▼ ]  [ worker-prod-1 ▼ ]
```

- If a group is selected, desktop uses `WorkerPool` from `crates/goble-core/src/worker_pool.rs` to pick a member.
- If a specific worker is selected, that worker receives the run.

### 8.4 Devices page

List all devices with:
- device name
- last seen
- fingerprint
- revoke button

## 9. Protocol Changes

Current `DesktopMessage` / `WorkerMessage` in `crates/goble-core/src/protocol.rs` assumes one desktop and one worker. Extend minimally:

- Add `DeviceId` to messages (or use client cert serial).
- Add `WorkerMessage::DeviceList` for sharing known workers between devices.
- Add `DesktopMessage::PushWorkerBundle` for sending worker certs.
- Add `DesktopMessage::AnnounceDevice` so devices can announce themselves to workers.
- Keep existing `RunAgent`, `ScheduleAgent`, `PushSecrets`, etc. but route them through the wallet-carrying device.

## 10. Security & Compliance

### 10.1 What is secure

- Cluster key never leaves user's wallet unencrypted.
- Worker certs are short-lived (rotate every N days).
- mTLS between every device and worker.
- No central server holds user data.
- Revocation via CRL.

### 10.2 What must change to be compliant

| Gap | Fix |
|---|---|
| `vault.json` uses empty passphrase in some paths | Always derive vault key from `ClusterKey` + user passphrase |
| SQLite store on device unencrypted | Encrypt store with device key + vault key |
| No audit log | Add `AuditLog` table in device store; log every command signed by device cert |
| No key rotation | Add `goble-cli identity rotate-worker-certs` + UI card |
| Backup is optional | Warn user until wallet is exported |

### 10.3 Compliance posture

- **GDPR**: ✅ user data stays on devices and owned workers.
- **SOC2**: ✅ with audit logs, key rotation, mTLS.
- **HIPAA**: ⚠️ requires at-rest encryption + BAA with cloud provider; technically feasible but not default.
- **DORA/NIS2**: ⚠️ requires documented incident response; achievable with backup/restore flows.

## 11. Implementation Phases

This is a multi-month plan. Phases are ordered to keep the product usable after each step.

### Phase 1 — Snapshot engine

**Objective:** every device and worker can create, encrypt, upload, and restore an object-storage snapshot.

Files:
- `crates/goble-core/src/snapshot.rs` (new) — format, encryption, manifest, S3/R2/MinIO/B2 providers; used by workers for upload/restore, not by UI.
- `crates/goble-core/src/encrypted_wallet.rs` — snapshot contains `IdentityWallet` for device restore.
- `crates/goble-core/src/cluster_key.rs` — derive snapshot encryption key from `ClusterKey`.
- `crates/goble-core/src/store.rs` — export/import store contents to/from snapshot format.
- `crates/goblin-worker/src/snapshot_runner.rs` (new) — worker-side periodic upload and startup restore.
- `crates/goble-cli/src/main.rs` — `goble snapshot restore` (device/admin) and `goble worker snapshot trigger` (worker admin).

Verification: create snapshot on device A, upload to R2, download on device B, restore, both see same agents and secrets.

### Phase 2 — Identity wallet as part of the snapshot

**Objective:** the wallet lives inside the snapshot; no raw cluster key is stored outside the encrypted snapshot.

Files:
- `crates/goble-core/src/encrypted_wallet.rs` — expand payload to `IdentityWallet`.
- `crates/goble-desktop/src-tauri/src/state.rs` — store and load from snapshot cache.
- `crates/goble-desktop/src/components/IdentitySettings.tsx` — new UI: sync passphrase, enable snapshot, restore from bucket.

Verification: enable snapshot on device A; configure bucket; device B restores from same bucket and has the same identity.

### Phase 3 — Certificate-based worker provisioning

**Objective:** remove pairing-code hash from worker bootstrap; use worker certs.

Files:
- `crates/goble-core/src/provision.rs` — generate `worker-bundle.json` with cert/key/CA.
- `crates/goblin-worker/src/main.rs` — add `--bundle` startup mode.
- `crates/goblin-worker/src/pairing.rs` — replace hash check with cert validation.
- `crates/goble-core/src/identity.rs` — ensure `ClusterCa::sign_worker` exists and is tested.

Verification: provision a VPS worker without pairing code; desktop connects via mTLS.

### Phase 4 — Device sync via snapshot

**Objective:** adding a new device means downloading the latest snapshot.

Files:
- `crates/goble-core/src/device_transfer.rs` (new) — optional QR/phrase shortcut for bucket credentials, but primary flow is restore from snapshot store.
- `crates/goble-desktop/src-tauri/src/device_commands.rs` (new) — Tauri commands for device pairing and restore.
- `crates/goble-desktop/src/components/DevicesPage.tsx` — device list.

Verification: export bucket credentials on device A; device B enters them and restores; both can operate the same workers.

### Phase 5 — Kubernetes cluster mode with snapshot tier

**Objective:** a cluster is a single Helm install that optionally uses the snapshot tier.

Files:
- `deploy/goblin/Dockerfile`
- `deploy/goblin/charts/goblin-cluster/` (Helm chart)
- `crates/goblin-worker/src/main.rs` — `--mode=cluster`, snapshot flags, K8s env detection.
- `crates/goblin-worker/src/state.rs` — load worker cert from snapshot; reconstruct state.
- `crates/goblin-worker/src/scheduler.rs` — leader election for cron scheduling.
- `crates/goble-desktop/src-tauri/src/cluster_commands.rs` (new) — generate Helm command.
- `crates/goble-desktop/src/components/ClusterInstallCard.tsx` — chat card.

Verification: install cluster in local k3s/kind with snapshot enabled; kill pod; new pod downloads snapshot and resumes schedules; agents still work.

### Phase 6 — Worker groups and runtime routing

**Objective:** tags/groups; `WorkerPool` used by desktop.

Files:
- `crates/goble-core/src/worker_pool.rs` — already exists; wire into desktop.
- `crates/goble-desktop/src-tauri/src/state.rs` — add group storage.
- `crates/goble-desktop/src/components/WorkerGroupCard.tsx`.
- `crates/goble-desktop/src/components/RuntimeSelector.tsx`.

Verification: create group with two workers; run agent three times; each run lands on a different worker.

### Phase 7 — Audit, rotation, and compliance hardening

**Objective:** enterprise-ready.

Files:
- `crates/goble-core/src/audit.rs` (new) — structured audit log, part of snapshot.
- `crates/goble-core/src/store.rs` — encrypted device store.
- `crates/goble-cli/src/main.rs` — `identity rotate-worker-certs`, `identity revoke-device`.
- `crates/goble-desktop/src/components/SecuritySettings.tsx`.

Verification: SOC2-style checklist test; revoke device; worker rejects revoked cert; audit log survives in snapshot.

## 12. What Not to Build

- **Central account server** — contradicts single-key, self-hosted design.
- **Automatic K8s autoscale** — out of scope until runtime state is migratable.
- **Shared database for workers** — unnecessary complexity; wallet + device store + per-worker SQLite is sufficient.
- **Docker-in-Docker isolation** — V8 isolate remains the long-term sandbox; containers are overkill for the worker itself.

## 13. Open Questions to Resolve During Implementation

1. How does a new device discover the list of workers without first importing the wallet?
   - Answer: it imports the wallet first; discovery is wallet-driven.
2. How do we handle a user losing the only device with the wallet?
   - Answer: mandatory backup export during onboarding; optional encrypted cloud backup to user's own S3/Nextcloud.
3. Should workers accept connections from any device cert or only from known serials?
   - Answer: any valid device cert signed by the CA, plus optional explicit allow-list.
4. How do devices agree on the latest CRL version without a central server?
   - Answer: devices gossip CRL updates through workers; last writer wins by version.

## 14. Verification Checklist

- [ ] Two desktops can run the same agent on the same worker after wallet sync.
- [ ] A worker can be re-provisioned with a new certificate without changing the cluster key.
- [ ] K8s worker pod delete + reschedule reconnects to desktop and **resumes schedules from cluster snapshot** if snapshot tier is enabled.
- [ ] Revoking a device prevents it from sending commands to any worker.
- [ ] Chat command "add worker on my VPS" produces the correct composer card and provisioning succeeds.
- [ ] Chat command "install cluster" produces a valid Helm command and the cluster worker appears in the UI.
- [ ] Snapshot can be enabled/disabled per cluster; pods share the same bucket configuration.

## 15. First Concrete Task

Implement **Phase 1: Snapshot engine**. This unblocks every other phase and gives the swarm a single source of truth that is not a relational database.

Failing test to start with:
```rust
// crates/goblin-worker/src/snapshot_runner.rs test
#[tokio::test]
async fn test_worker_snapshot_restore_from_r2() {
    let tmp = tempfile::tempdir().unwrap();
    let worker_db = tmp.path().join("worker.db");
    let store = TaskStore::open(worker_db.clone()).unwrap();
    let state = AppState::new(WorkerId::generate());
    state.set_store_path(worker_db).unwrap();

    // seed some data
    let agent = AgentSpec::new("demo", "do nothing");
    state.store_agent(agent.clone());

    let cluster_key = ClusterKey::generate();
    let provider = R2SnapshotProvider::from_env().unwrap();
    let runner = SnapshotRunner::new(state.clone(), provider, cluster_key);

    runner.upload_now().await.unwrap();

    // simulate fresh worker
    let fresh_state = AppState::new(WorkerId::generate());
    let fresh_db = tmp.path().join("fresh.db");
    fresh_state.set_store_path(fresh_db).unwrap();
    let fresh_runner = SnapshotRunner::new(fresh_state.clone(), provider, cluster_key);
    fresh_runner.restore_latest().await.unwrap();

    let restored = fresh_state.agents.lock().get(&agent.id).cloned();
    assert!(restored.is_some());
}
```

Implementation order:
1. Define `Snapshot` and `SnapshotProvider` in `crates/goble-core/src/snapshot.rs`.
2. Add S3/R2/MinIO/B2 providers behind a trait.
3. Add `SnapshotRunner` in `crates/goblin-worker/src/snapshot_runner.rs` with periodic upload and startup restore logic.
4. Add `goble-cli snapshot restore` and `goble worker snapshot trigger` commands.
5. Run `cargo test --workspace`.
6. Commit, push, open PR.
