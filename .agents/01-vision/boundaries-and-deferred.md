# 01 — Boundaries & deferred scope

**Status:** `[x]` settled
**Owns:** the in-scope-now vs later line. No code.

## In scope now (target architecture)

These are the shape we build toward in the next iterations:

- **Single workspace** (local **or** remote) with the full packaging (agents, shared secrets/API keys, agent-editable TOML, plugins, workflows, `remember`, personas, deep-research).
- **The local/remote router** and the **first-run flow**:
  1. No model key → banner: *"You don't have any key configured, please click here to configure a model key."*
  2. After a key is configured → prompt: *"Do you want to create a workspace local or remote?"*
  3. **Remote** → a custom composer appears to write secrets, then the agent connects to the server, downloads the package, self-configures there by reading the TOML, and the conversation is **routed remote**.
- **Agent runtime** (harness): persona, continuous state, compaction to infinity, sub-agents for routines, tool registry, dynamic MCP install, LLM/model resolution, per-agent CWD + sandbox, inter-agent communication, `remember`, deep-research.
- **Observability**: executions, traces, logs surfaced from the runtime.
- **Renderer**: our own wgpu/Rust chat renderer; the ability to rate the renderer onto a remote host's terminal.

## Deferred (direction recorded, not built)

| Area | Doc | Why deferred |
| --- | --- | --- |
| **Multiple workspaces** | [`../03-workspace-model/workspace.md`](../03-workspace-model/workspace.md) | Product/business logic still settling; single workspace first. |
| **Threads UI** | [`../08-threads-deferred/README.md`](../08-threads-deferred/README.md) | The worker-hosted thread-messaging server needs the workspace model first. |
| **Mobile client** (Kotlin, Android/iOS) | [`../09-mobile-deferred/README.md`](../09-mobile-deferred/README.md) | Requires the worker-as-workspace + Tailscale path to be real. |

## Explicitly out of scope

- Replacing the working `goble-core`/`goble-desktop-service`/`goblin-worker` backend with the grok-build harness wholesale — we **reuse** the harness crates, we do **not** vendor grok-build into the product shell.
- Building a mobile app in this milestone.
- Full threads/chat feature parity from the legacy React app (`goble-desktop`) — the native shell is the product shell now.
