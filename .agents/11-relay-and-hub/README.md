# 11 — Relay and hub

**Status:** `[~]` core libraries landed
**Owns:** reaching any worker from any device, and running work on workers without a human at the keyboard
**Depends on:** [`../02-first-run-and-routing/remote-bootstrap.md`](../02-first-run-and-routing/remote-bootstrap.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md), [`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md), [`../09-mobile-deferred/README.md`](../09-mobile-deferred/README.md)

## What this subsystem is

Two services that turn the single-machine product into a fleet:

- **Relay** — the always-on address workers dial out to and clients dial in to, when neither has a
  public address. It carries bytes and routes by id; it is not a renderer and not a database. The
  in-repo `goble-gateway` is its seed, not its final shape.
- **Hub** — the automation plane. It accepts events (GitHub, Slack, Linear, Discord, schedules, HTTP,
  MCP), deploys versioned workflows, leases runs to workers, and records the audit trail.

The client/worker protocol already exists (`goble-daemon-protocol`, mTLS WebSocket). The relay adds
reachability; the hub adds triggers and dispatch. Neither introduces a second execution model: a
hub run is a normal harness session with a normal per-turn ledger, so it can be forked, replayed and
reverted like any conversation.

## Boundaries

| Layer | Job | Never does |
|---|---|---|
| Relay | TLS termination, device/worker authentication, opaque frame routing, quotas, revocation | Parse daemon frames, store transcripts, hold model keys |
| Hub | Trigger intake, workflow revisions, queue/leases, target selection, audit | Run the harness itself, hold the user's model keys at rest |
| Worker | Run the harness and its PTY, keep the ledger, enforce grants | Expose an inbound port, trust a hub event without a signature |
| Client | Render, send input, approve | Become the source of truth for a run's state |

## Documents

- [`relay-auth.md`](relay-auth.md) — device identity, passkey sessions, QR pairing, end-to-end
  encryption, and why the gateway's PIN goes away.
- [`hub.md`](hub.md) — triggers, workflow revisions, dispatch and leases, sandbox lifecycle,
  approvals, audit, API.

## What exists today

- `crates/goble-gateway` — loopback-by-default WebSocket relay with a PIN-gated browser session and
  a shared worker bearer token; frames copied verbatim; in-memory state.
- `crates/goblin-worker/src/gateway_client.rs` — the worker's outbound connection with reconnect
  backoff, so a sandbox behind NAT needs no inbound port.
- `crates/goble-web` — the browser terminal (WASM, vt100, canvas) the gateway serves.
- `goble-remote` providers (`e2b`, `modal`, `daytona`, `box`, `docker`, `ssh`) with creation
  timeouts and provider-side auto-stop/auto-pause.
- `goble-cli ctl` and `goble-mcp` — the control verbs and tool names the hub will call.
- `goble-core` identity primitives: `ClusterKey`, device certificates, `EncryptedWallet`, worker
  provisioning bundles.
- `crates/goble-relay` — the relay's core: QR/SAS pairing and credential delivery, WebAuthn
  verification, rotating account sessions with step-up, the E2EE frame layer, and a durable
  device/worker/revocation registry. 68 tests.
- `crates/goble-hub` — the hub's core: workflow parsing and immutable revisions, webhook signature
  verification and delivery dedupe, the run/lease state machine, tag/health target selection, the
  sandbox reaper rule, and a signed audit chain with a Merkle root. 62 tests.

Both crates are workspace members and `cargo check --workspace --all-targets` passes. The gateway and
worker binaries are not wired to them yet; the relay still serves its fenced PIN development path.

## Tasks

- [~] Define the relay wire protocol and the device certificate format (`relay-auth.md`).
- [~] Replace the gateway PIN with passkey sessions and QR pairing (`relay-auth.md`) — the modules
      and tests landed; the gateway is not wired yet.
- [x] Move the relay's state to a durable registry (workers, devices, revocations, quotas)
      (`relay-auth.md`) — quotas are not in the schema yet.
- [x] Define the hub's database and the run/lease state machine (`hub.md`).
- [x] Add the GitHub, Slack, Linear and Discord intake adapters (`hub.md`).
- [x] Implement workflow revisions and deployment validation (`hub.md`).
- [~] Wire target selection to the existing runtime targets and sandbox providers (`hub.md`) —
      tag/health/load selection and the reaper rule are in; provider calls are not.
- [~] Link every hub run to its harness session and ledger so it can be forked/replayed/reverted
      (`hub.md`) — the session id column and setter exist; fork/replay/revert are not wired.
- [ ] Add the hub API and its MCP mirror (`hub.md`).
