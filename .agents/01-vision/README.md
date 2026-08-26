# 01 — Vision

**Status:** `[x]` direction settled (details evolving)
**Owns:** product goals + principles. No code.
**Depends on:** [`../00-README.md`](../00-README.md)

## North star

Goble lets **anyone build complex things from natural language**. You describe an agent, and the agent composes tools, MCP servers, and other agents in a workspace to get the work done. It is an "agent control center", not a chatbot.

## Goals

- Creatable agents that complete user-defined tasks; each has a **persona and a long-lived state** so you can keep talking to the same agent and its conversation **compacts indefinitely**.
- Agents that can **search and download MCP servers at runtime**, each get their **own CWD**, and can **communicate with other agents** in the same workspace.
- A **workspace** that packages agents + secrets + API keys + an agent-editable TOML + plugins + workflows + `remember` + personas + deep-research. It runs locally or on a **remote host**.
- A **router** so a conversation can start local and move to a remote workspace (or vice-versa) without losing continuity.
- The **local machine can also act as a worker** (via Tailscale) for the future mobile app; a worker hosts agents + their crons + secrets + a thread-messaging server, i.e. *a worker is a workspace*.
- A **custom, from-scratch renderer** on `wgpu`/Rust (no UI libraries) for maximum performance and control.

## Principles

1. **Our renderer, their harness.** The agent execution engine is reused from `~/Projects/grok-build` (see [`04-agent-runtime/harness-reuse-map.md`](../04-agent-runtime/harness-reuse-map.md)); the chat renderer is ours.
2. **No UI libraries.** Everything paints through `goble-ui`/`goble-ui-hot` on `wgpu`.
3. **Workspace is the deployment unit.** An agent never floats free of a workspace, and a workspace is either local or remote.
4. **Continuity over teleporting.** Local↔remote routing keeps the conversation state and the agent identity coherent.
5. **The agent owns its config.** The workspace TOML is something the agent itself reads and can edit.
6. **Sub-agents for routines.** The user keeps typing; sub-agents do the work and report back.

## Boundaries with the existing code

The repo already has a working backend (`goble-core` + `goble-desktop-service` + `goblin-worker`) and a native shell (`app/` = `goble-app` + `goble-ui-hot`). This folder *extends* the architecture — it does not replace the working pieces. See [`boundaries-and-deferred.md`](boundaries-and-deferred.md).

## Related

- [`../02-first-run-and-routing/README.md`](../02-first-run-and-routing/README.md) — the first-run flow and the local/remote router
- [`../03-workspace-model/README.md`](../03-workspace-model/README.md) — the workspace as the unit of deployment
- [`../10-platform-and-performance/README.md`](../10-platform-and-performance/README.md) — perf/rendering principles
