# 03 — Workspace as an addressable unit

**Status:** `[~]` shape agreed
**Owns:** the workspace identity/address and its deployment (local vs remote vs worker-as-workspace)
**Depends on:** [`README.md`](README.md)

## Problem

Everything in Goble is scoped to a workspace (agents, secrets, TOML, plugins, workflows, memory, personas, deep research, threads). The rest of the system must be able to refer to "the workspace" without caring whether it runs on this laptop or on a VPS — and the router must be able to move a conversation between them.

## Workspace identity

```rust
struct Workspace {
    id: WorkspaceId,          // stable uuid
    name: String,
    kind: WorkspaceKind,      // Local | Remote { address, worker_id }
    root: PathBuf,            // CWD root for the workspace's agents
    config: WorkspaceConfig,  // the agent-editable TOML (see shared-secrets-and-toml.md)
}
```

- The **id** is the stable handle. The **kind** can change (local → remote) without the id changing; that is what keeps agents, conversations and state coherent across a migration.
- The **root** gives every agent a sane base directory; each agent gets its own sub-CWD under it (see [`../04-agent-runtime/sandbox-and-cwd.md`](../04-agent-runtime/sandbox-and-cwd.md)).

## Deployments

| Kind | Where it runs | Transport |
| --- | --- | --- |
| `Local` | in-process / local worker on this machine | none (or loopback) |
| `Remote` | a goblin worker on a remote host | WebSocket over mTLS |
| **self-as-worker** | this machine acting as a worker for the mobile app | Tailscale |

## A worker is a workspace

The goblin worker already bundles the runtime, crons, secrets and (planned) the thread-messaging server. That is the same package as a workspace. Therefore:

- A **remote workspace** == a worker that was provisioned with a workspace package (see [`../02-first-run-and-routing/remote-bootstrap.md`](../02-first-run-and-routing/remote-bootstrap.md)).
- **Self-as-worker (Tailscale)** == running the same worker package locally and exposing it to the mobile client; that is the deferred path in [`../09-mobile-deferred/README.md`](../09-mobile-deferred/README.md).

## Workspace home (`~/.goble`)

Every user has a hidden home `~/.goble` on their machine that mirrors the `~/.grok`
layout, but its **content depends on whether the workspace is local or remote**:

- **Base (universal)** — created for *every* user: `principal_id` + `auth.json`
  (identity), `config.toml` (agent-visible config, with the remote address when the
  workspace is remote), `README.md`, `version.json`, `sessions/`, `logs/`,
  `principals/<id>/`, `docs/user-guide/`, `relocations/`.
- **Workspace payload (local only)** — materialized only when the workspace runs on
  this machine (local / self-as-worker): `bundled/{agents,roles,personas,skills}`,
  `worktrees/`, `threads/`, `downloads/`, `bin/`, `vendor/`, `completions/`,
  `marketplace-cache/`, `plugins/`, `skills/`, `workflows/`, and the local store
  `goble_store.sqlite`.

A **remote-only** user therefore has a minimal home: identity + essential data only,
because the workspace (agents, skills, workflows, store) lives on the remote worker.
`config.toml` is the agent-visible config; every principal with access, plus the
grants they hold, is recorded in the store (`access_grants`; see
[`shared-secrets-and-toml.md`](shared-secrets-and-toml.md)). The local user is one
principal (`PrincipalId::default_user()`); the router picks the worker/workspace, and
that workspace's home carries all its principals and grants. When the machine is a
remote worker the same home ships as the workspace package.

## Multi-workspace (deferred)

Supporting more than one workspace per deployment is recorded here as a non-goal for now. When we add it, the only structural change is a list of workspaces instead of one; the per-workspace packaging stays identical.

## Tasks

- [ ] Introduce a `Workspace` type (id, kind, root, config) and persist it.
- [ ] Allow a workspace's `kind` to be promoted local → remote while keeping the same `id`.
- [ ] Make the worker provisioned in a workspace bundle the remote workspace record (endpoint, TLS bundle, worker id).
