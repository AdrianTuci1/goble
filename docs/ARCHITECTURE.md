# Goble P2P Cluster Architecture

## Overview

Goble is an open-source autonomous-agent platform. This document describes the peer-to-peer (P2P) security and clustering model that replaces per-worker pairing and avoids any central cloud service.

Goals:

- No central server is required for authentication or data storage.
- A user can install the desktop client on many machines and control the same workers.
- A user can add many workers without pairing each one manually.
- A user can invite other people with limited permissions (admin, operator, viewer).
- Cross-device migration is possible by exporting an encrypted cluster bundle.

## Core Concepts

### Cluster Identity

When the user creates a cluster, the desktop client generates a self-signed root CA:

- `root_ca_key.pem` — kept encrypted on the user's device, never leaves the device except inside an encrypted backup.
- `root_ca_cert.pem` — distributed to every worker and device so they can verify each other.

The root CA is the single source of trust for the whole cluster.

### Roles

Every certificate issued by the root CA contains a role extension. Roles are hierarchical:

| Role | Permissions |
|------|-------------|
| `Owner` | Full control. Can rotate the CA, revoke certificates, invite owners. |
| `Admin` | Add/remove workers and devices, configure agents, workflows, MCP, LLM. |
| `Operator` | Run agents, view history, inspect executions. Cannot change cluster topology. |
| `Viewer` | Read-only access to dashboards and history. |
| `Worker` | Runs tasks, reports status, synchronizes state with other workers. |

A device certificate has one role. A worker certificate always has role `Worker`.

### Device Certificates

Each device (desktop, laptop, future mobile client) that joins the cluster receives a device certificate signed by the root CA:

- Subject common name: device UUID.
- Role extension.
- Validity: default 365 days, renewable by any Admin or Owner.

The device keeps its private key in the OS keychain or encrypted local storage.

### Worker Certificates

During worker installation the desktop client:

1. Generates a private key on the target VPS (or locally and injects it securely via SSH).
2. Creates a CSR and signs it with the root CA.
3. Installs the worker certificate + CA certificate + the list of peer workers.
4. The worker starts accepting only mTLS connections where the client cert is signed by the cluster CA and has role Admin, Owner, or Worker.

## Network Model

### Client → Worker communication

All communication between a desktop client and a worker uses mTLS over WebSocket or HTTPS.

```
Desktop client (device cert)
        |
        | wss://worker.example.com:8787/ws
        | mutual TLS
        v
      Worker (worker cert)
```

The worker verifies:

- The client certificate chain ends at the cluster root CA.
- The role in the certificate allows the requested operation.
- The certificate is not expired and not revoked.

### Worker → Worker communication

Workers in the same cluster gossip small state updates:

- ACL changes (new or revoked certificates).
- Agent and workflow definitions.
- Task delegation when one worker is overloaded.
- Health heartbeats.

Every inter-worker message is signed with the sender's worker private key. The receiver verifies the signature against the sender's certificate.

```
  Worker A <-----> Worker B
      \              /
       \           /
        \        /
         Worker C
```

## Cluster Coordination

Workers elect a coordinator using a deterministic rule (lowest worker ID among healthy peers). The coordinator is not a separate binary; any worker can become coordinator.

Responsibilities of the coordinator:

- Accept writes for shared state (agents, workflows, ACL updates) and replicate them to peers.
- Resolve conflicts using wall-clock timestamps + vector clocks if clocks drift.
- Maintain the canonical list of active workers.

If the coordinator fails, remaining workers detect it through missed heartbeats and elect a new coordinator automatically.

### Conflict resolution

Shared state uses a simple last-write-wins model with per-key vector clocks. If two devices edit the same agent concurrently, the coordinator returns both versions and the desktop UI asks the user to choose. This is acceptable for an MVP and avoids operational complexity.

## Access Control and Revocation

### Certificate Revocation List (CRL)

The desktop client maintains a signed CRL file containing serial numbers of revoked device/worker certificates. Workers cache the CRL and refresh it from any peer that has a newer version.

CRL is signed by the root CA public key. Workers accept a newer CRL only if it has a higher version number and a valid signature.

### Short-lived certificates

To reduce the impact of a leaked certificate, device certificates can optionally be issued with short lifetimes (e.g. 30 days) and require renewal by an Admin or Owner. Worker certificates can have longer lifetimes because they are harder to steal when mTLS is enforced.

## Cross-Device Recovery

A user can export a cluster bundle from the desktop client:

```
goble-cluster-2026-07-26.goble
```

The bundle contains:

- Root CA private key (encrypted).
- Device certificates for all known devices.
- Worker list with addresses and certificates.
- Agents, workflows, MCP servers, LLM settings, vault secrets.
- CRL and role policy.

Encryption:

- User sets a master password.
- Argon2id derives a 256-bit key.
- ChaCha20-Poly1305 encrypts the bundle.

On a new computer the user imports the bundle, enters the master password, and is immediately connected to the same cluster. The new device receives its own fresh device certificate signed by the root CA from the bundle.

## Adding a New Device

Two flows are supported:

### Flow A — Backup import

1. On old device: Settings → Profile → Export cluster bundle.
2. Transfer the `.goble` file through any secure channel (USB, encrypted email, password manager).
3. On new device: Settings → Profile → Import cluster bundle.
4. New device generates a fresh device certificate and replicates it to workers.

### Flow B — Invite link

1. Owner or Admin opens Settings → Devices → Invite device.
2. Desktop generates a short-lived invite token + a device certificate pre-approved by the CA.
3. Token is shared through a secure channel (QR code, paste).
4. New device uses the token to register itself and pull the CA cert + worker list from any reachable worker.

Invite tokens expire after 1 hour or after first use.

## User Stories

### Single user, multiple workers

- Alice installs Goble desktop.
- Desktop generates a root CA.
- Alice installs worker 1 on VPS A; worker 1 receives a worker certificate.
- Alice installs worker 2 on VPS B; worker 2 receives a worker certificate and the address of worker 1 as a peer.
- Worker 1 and worker 2 discover each other and elect a coordinator.
- Alice can see both workers in Settings → Workers and run agents on either.

### Multiple devices, same cluster

- Alice exports a cluster bundle from her laptop.
- She imports the bundle on her desktop.
- The desktop gets a fresh device certificate and connects to the same workers.
- Changes made on one device are visible on the other after the next sync.

### Inviting a team member

- Bob (Owner) invites Charlie as Operator.
- Desktop generates a device certificate with role `Operator` and an invite token.
- Charlie installs Goble desktop and enters the invite token.
- Charlie can run agents and view history but cannot add workers or change settings.

### Revoking access

- Bob revokes Charlie's certificate in Settings → Devices.
- Desktop updates the CRL and pushes it to all workers.
- Workers reject Charlie's device certificate on the next request.

## Threat Model

### What we protect against

- Eavesdropping on client–worker traffic (mTLS).
- Rogue devices trying to control workers (role-based client cert verification + CRL).
- Worker impersonation (worker certificates signed by CA, message signatures).
- Backup theft (encrypted with user-controlled password).

### What we do not protect against

- Theft of the root CA private key from an unlocked device. Mitigation: encrypt with OS keychain, require master password for export.
- Compromised worker host. Mitigation: attacker can read worker certificate but cannot generate new certs; revoke the worker cert and rotate.
- Social engineering. Mitigation: invite tokens are short-lived and single-use.

## Implementation Phases

1. **Identity module** — CA, role certificates, CRL, certificate store.
2. **mTLS worker** — worker accepts only mTLS, verifies client role.
3. **Desktop cluster setup** — generate CA, install worker with cert, connect over mTLS.
4. **Encrypted backup/restore** — export/import `.goble` bundle.
5. **P2P sync and coordinator election** — worker gossip, shared state replication.
6. **Multi-device and invites** — device certificates, invite tokens, CRL UI.

## Open Decisions

- Certificate lifetime default: 365 days for devices, 365 days for workers.
- Sync protocol: gossip over WebSocket with signed JSON messages.
- Coordinator election: lowest worker ID; health timeout 30 seconds.
- Storage format for shared state: SQLite on each worker with deterministic conflict resolution.
