# Principals and Access

A **principal** is an identity that can reach a workspace: the local user, a remote operator, or a service account. Goble records **every** principal that has access and the **grants** each holds, so a workspace's home is the full picture of who may do what — not a single identity.

---

## The Model

- **Principal** — `id`, `kind` (`user`, `slack-channel`, `team`, `client`), `name`, `created_at`.
- **Access grant** — `principal_id`, `grant`, `scope`, `created_at`. A grant is an action (e.g. `deploy`, `run_command`, `read_store`) applied to a scope (e.g. a workspace, an agent, a thread).
- **MCP accounts** — per-principal MCP credentials, linked by `secret_ids` to the credentials store.

The local user is one principal, recorded in `~/.goble/principal_id` and `~/.goble/auth.json`. Each principal's context can live under `~/.goble/principals/<id>/`.

---

## Listing Access

The agent can report who has access and what they may do:

1. Ask "who has access to this workspace?" — the agent calls `principals`.
2. The `principals` tool returns every principal with its kind/name and the grants (`grant:scope`) it holds. **No secret values** are returned.

---

## Granting and Revoking

Access is managed through the app:

- **Grant** — add a principal and assign grants (e.g. `deploy`, `run_command`, `read_store`) with a scope.
- **Revoke** — remove a principal's grant, or remove the principal entirely.

Grants are enforced where the agent uses a resource: `run_command` for shell, `deploy_agent`/`deploy_workflow` for deployment, the store for searches.

---

## Why It Matters

When the machine is a remote worker, the **same** `~/.goble` home ships with the workspace package — including principals and their grants — so access control travels with the workspace rather than being re-derived on the worker.

---

## Related

- [Credentials](04-credentials.md) — per-principal secrets.
- [Workspaces](02-workspaces.md) — the home a workspace carries.
