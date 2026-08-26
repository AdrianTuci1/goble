# 03 — Multiple agents per workspace

**Status:** `[~]` model agreed; agent lifecycle/state pending
**Owns:** how agents coexist in one workspace
**Depends on:** [`README.md`](README.md), [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md)

## Problem

A workspace runs several agents with distinct persona/purpose (e.g. a coding agent, a reviewer, an ops agent). They share the workspace's secrets and TOML, but each behaves as its own "person" with its own long-lived conversation and its own working directory.

## Model

```rust
struct Agent {
    id: AgentId,
    workspace_id: WorkspaceId,
    name: String,
    persona: PersonaId,          // see ../04-agent-runtime/personas.md
    cwd: PathBuf,                // subdir under the workspace root (per-agent)
    state: AgentState,           // Draft | Deployed | Running | Paused | Error
    config_ref: ...,             // what this agent reads from the shared TOML
}
```

Key properties:

- **Shared config, isolated state.** All agents read the same workspace TOML and use the same vault secrets; but each has its **own conversation/memory** and its **own CWD**.
- **Agents talk to each other** within the workspace (see [`../04-agent-runtime/inter-agent-communication.md`](../04-agent-runtime/inter-agent-communication.md)).
- **Each agent is created from natural language** ("create a coding agent that reviews PRs…"), and the harness/TOML define what tools/plugins the agent can use.

## Gaps vs current code

- `goble-core::agent::AgentSpec` covers `id, name, description, prompt, tools, triggers, mcp_ids` — enough for a flat agent but no `workspace_id`, persona, CWD, or long-lived state.
- There is no per-agent conversation/memory store keyed on the agent (the product's "talk continuously, compact to infinity" needs it).

## Tasks

- [ ] Add `workspace_id`, `persona`, `cwd` and a per-agent conversation/memory key to the agent model.
- [ ] Persist per-agent state so an agent survives restarts and retains its conversation.
- [ ] Let agents be created/edited from natural language and bound to workspace config.
