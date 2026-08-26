# 04 — Sub-agents (routines)

**Status:** `[~]` partial: goblin-worker runtime + `xai-grok-subagent-resolution` to reuse
**Owns:** spawning short-lived agents to run routines without blocking the main chat
**Depends on:** [`README.md`](README.md)

## Problem

The user keeps typing while the agent works. If the harness blocks the main chat loop to run a long task, the conversation stalls. Routines (a review, a search, a build) should run as **sub-agents** that report back, so the main chat stays alive.

## Model

- **Main agent** = the persona you talk to; owns the conversation.
- **Sub-agent** = a disposable agent spawned for one routine. It gets its own prompt/context (derived from the main agent), its own CWD, runs to completion, and returns a result/events to the main agent.
- The main agent can spawn several sub-agents; each can itself spawn sub-agents (bounded depth).

```rust
struct SubAgent {
    id, parent: AgentId,
    routine: String,
    prompt: String,       // derived from parent persona + task
    cwd: PathBuf,
    budget: TokenBudget,  // max call count / tokens (see agent_budget semantics)
    status: Running | Done(result) | Failed(err),
}
```

## Why sub-agents (not just tools)

- They have their **own context/memory** for the routine, so a long task doesn't bloat the main transcript.
- They run **concurrently**, so the main chat keeps accepting input.
- They give the user a **visible "routine" card** (see the existing crons/routines UI) with progress and a result.

## Reuse

- `xai-grok-subagent-resolution` (definition/runtime/prompt/resume) is the reference — see [`harness-reuse-map.md`](harness-reuse-map.md).
- The existing `goblin-worker::agent_runtime` already has a subagent/tool runtime we can extend.

## Boundaries

- Sub-agents are scoped to the workspace and inherit its secrets/TOML (same rules as [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).
- They are **not** "the agent you talk to" — they don't own the long-lived persona state.

## Tasks

- [ ] Add a `SubAgent` type with bounded budget + lifecycle.
- [ ] Spawn sub-agents from the harness so the main chat loop stays responsive.
- [ ] Surface routine progress/results in the renderer.
