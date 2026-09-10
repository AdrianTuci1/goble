# 04 — Inter-agent communication

**Status:** `[ ]` not started
**Owns:** how agents in the same workspace talk to each other
**Depends on:** [`README.md`](README.md), [`../03-workspace-model/multiple-agents.md`](../03-workspace-model/multiple-agents.md)

## Problem

Agents in a workspace must collaborate: a coding agent hands off to a reviewer, an ops agent reports to a planner. This requires a channel plus a shared contract so one agent can address, ask, and get an answer from another.

## Model

- **Addressing:** by `AgentId` (workspace-scoped). An agent can send a message or a **task request** to a peer.
- **Channel:** a workspace-scoped message bus. Locally it's in-process; remote it's the same bus over the worker/thread-messaging transport.
- **Contract:** messages are structured (e.g. `Message { to, from, kind: Text | TaskResult | ToolOffer }`), not free-form-only.
- **Visibility:** the user can see agent↔agent activity (renderer shows which agent is talking to which) — ties into [`../07-observability/README.md`](../07-observability/README.md).

## Rules

- Agents inherit the workspace's secrets/config — communication never bypasses that boundary.
- No agent can call another's private state; only its API (send a message / request a task).
- Loops must be bounded (depth/reply limits) to prevent runaway agent churn.

## Relation to threads

The inter-agent bus and the **thread-messaging server** (deferred, [`../08-threads-deferred/README.md`](../08-threads-deferred/README.md)) should be the *same* channel concept — a worker hosts the messaging server, and agents as well as human thread participants are all participants.

## Tasks

- [ ] Define the workspace message-bus contract (addressing, message kinds).
- [ ] Implement the local in-process channel; reuse the worker transport for remote.
- [ ] Enforce loop bounding and show agent↔agent activity to the user.
