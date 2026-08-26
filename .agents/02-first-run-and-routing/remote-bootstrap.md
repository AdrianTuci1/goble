# 02 — Remote bootstrap

**Status:** `[ ]` not started
**Owns:** standing up a remote workspace and switching routing to it
**Depends on:** [`router-local-vs-remote.md`](router-local-vs-remote.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md), [`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md)

## Problem

The user chooses **remote**. The app must turn a remote host into a workspace: connect, transfer the package, configure it from the workspace TOML, verify it, then point the conversation at it.

## Sequence (target)

```mermaid
sequenceDiagram
  participant U as User
  participant A as Goble app
  participant R as Remote host (worker)

  U->>A: choose "remote" workspace
  A->>A: show custom composer; user enters secrets
  U->>A: submit secrets
  A->>R: connect + authenticate (mTLS / SSH) 
  A->>R: transfer/install the workspace package
  A->>R: ship workspace TOML + vault secrets
  R->>R: self-configure (read TOML, resolve providers/models)
  R-->>A: ack + publish workspace endpoint
  A->>A: mark conversation "routed remote"
  A-->>U: conversation continues via remote workspace
```

## Rule: who reads the TOML

The **remote harness** reads the workspace TOML on the remote host and configures itself there. The TOML is the single source of truth for providers, models, tool/plugin selection, and API-key *references* (never the keys themselves). This lets the TOML be agent-editable without leaking secrets.

## Outputs

- A **remote workspace record** (address, worker id, workspace id, TLS bundle) so the router can reference it.
- The conversation flips from `local` to `Remote { worker_id, address }`.

## Reuse

This rides on the existing worker pairing/install path in `goble-desktop-service` (`pair_worker`, `WorkerClient::connect`, `cluster_helm_install`, `install_worker_ssh`) plus the mTLS bundle signing in `goble-core`. The remote *package* is new: what gets shipped and how it self-configures.

## Tasks

- [ ] Define the "workspace package" to ship to a remote host (harness + TOML + secrets).
- [ ] Add the remote self-configuration step (read TOML → resolve providers/models).
- [ ] Reuse the existing pairing/SSH/helm install path to bring up the remote workspace.
- [ ] Publish a workspace endpoint the router can point a conversation at.
