# 11 — Multi-tenant worker

**Status:** `[~]` design / slice 1
**Owns:** running a single `goblin` worker that serves multiple desktops, sharing agents but keeping per-user conversation history.
**Depends on:** [`../02-first-run-and-routing/router-local-vs-remote.md`](../02-first-run-and-routing/router-local-vs-remote.md), [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)

## Goal

A single `goblin` worker runs on a VPS as a daemon. Multiple Goble desktops connect to it, each with its own identity. The worker:

1. runs continuously (daemon / systemd / container), not spawned per connection;
2. accepts multiple simultaneous authenticated connections (mTLS WebSocket);
3. identifies the desktop/user from the client certificate;
4. stores shared agents, workflows, teams, MCP servers in the workspace store;
5. stores execution traces / chat transcripts per user;
6. executes agents on behalf of the user that initiated the request.

## Why mTLS WebSocket, not SSH spawn

SSH spawn is single-session by nature: one SSH session → one `goblin --ssh-proxy` process. That is perfect for one owner spawning their own worker, but it cannot share live state between users. For multi-tenant sharing we need a long-lived worker that many clients can reach. mTLS gives each desktop a strong identity (client certificate signed by the cluster CA) without sharing SSH credentials.

## Identity model

- One cluster CA per workspace (existing `ClusterCa` / `ClusterKey`).
- The CA lives on one desktop that creates the cluster; it can export/import the cluster key to other owner devices.
- Each desktop gets a device certificate signed by the CA (`ClusterRole::Device`).
- The worker gets a worker bundle (server cert + CA trust) signed by the CA.
- When a desktop connects over WSS, the worker validates the client certificate and extracts the desktop/user id from it (e.g., certificate serial or CN).
- The same cluster key can be shared across a small team today; later a proper invite/grant flow can issue per-user certificates without exposing the CA key.

## Protocol changes

Add an optional `desktop_identity` / `user_id` field to the envelope level, or derive it from the mTLS connection. The cleanest approach is:

- Keep `DesktopMessage` / `WorkerMessage` payload unchanged.
- On the worker side, attach the authenticated `Identity` to each open WebSocket connection in a `ClientSession`.
- Add `DesktopMessage::Identify { user_id, token }` only as a fallback for non-mTLS transports; mTLS connections skip it.
- Add `user_id` to trace/chat persistence so each user sees only their own history.

## Store changes

- `agents`, `teams`, `workflows`, `mcp_servers`, `secrets` stay global to the workspace (shared).
- `executions`, `traces`, `threads`, `chats` gain a `user_id` / `desktop_id` column.
- List/query methods filter by the calling user unless the caller is explicitly browsing shared workspace entities.
- The worker's `Runner` receives the calling user identity so events are routed to the right desktop session.

## Connection model on the worker

```text
Desktop A  ──WSS+mTLS──┐
Desktop B  ──WSS+mTLS──┼──> goblin worker  ──> shared workspace store
Desktop C  ──WSS+mTLS──┘
```

- The worker keeps a map `user_id -> ClientSession` for routing events back.
- A trace id is globally unique, so `AgentLog` / `AgentFinished` events go to all sessions that are subscribed to that trace; in practice the initiating session.
- Scheduled tasks (routines) run under the worker identity but execute agents for users that subscribed to them.

## Lifecycle

1. Admin/owner creates a cluster CA on their desktop.
2. Owner generates a worker bundle for the remote host and deploys it (SSH install, Helm, manual).
3. Worker starts with `--tls-bundle <bundle>`, listens on a port (default 8787).
4. Each desktop imports the cluster key (or receives an invite) and gets a device certificate.
5. Desktop connects via WSS; worker accepts only valid client certs, identifies user.
6. Desktop sends `PairRequest` with worker id; worker stores the desktop identity as authorized and emits `Paired`.
7. From then on, `RunAgent` / `RunAgentForThreadReply` etc. run under that user.

## SSH still supported, but different use case

- SSH-spawn remains for the single-owner model (no open port, one process per user).
- Multi-tenant is mTLS daemon on an open port.
- The same binary supports both modes via flags: `--ssh-proxy` (single-session) or `--tls-bundle` (multi-tenant daemon).

## Implementation slices

1. **Worker mTLS connection identity**: parse client cert, attach user id to each WebSocket connection, store authorized desktop identities.
2. **Per-user transcript store**: add `user_id` to executions/traces/threads/chats and filter by caller.
3. **Shared workspace entities**: ensure agents/teams/workflows/mcp/secrets are not filtered by user.
4. **Multi-client routing**: worker routes `WorkerMessage` events back to the correct desktop session(s).
5. **Invite/grant flow**: issue device certificates to new users without exporting the cluster key.
6. **Deployment helpers**: systemd unit / Helm update / install script that runs worker as daemon with mTLS.

## Gaps vs current code

- `goblin-worker` currently stores one `pairing_hash` globally and one `desktop_identity`. Needs per-user pairing/authorization.
- `WorkerClient::connect` already supports mTLS bundles. Needs to send client certificate and verify server identity.
- `DesktopState::pair_worker` already signs worker bundles. Needs to sign device certificates for additional users.
- Store tables need `user_id` columns.
- The `ssh_proxy` mode is single-session; it can be left as-is for single-owner use.

## Next immediate slice

Start slice 1: make the worker accept mTLS WebSocket connections, extract the client identity, and keep a `ClientSession` map keyed by user id, replacing the single global `desktop_identity` / `pairing_hash`.
