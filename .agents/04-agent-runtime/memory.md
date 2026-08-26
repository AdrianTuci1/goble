# 04 — Memory (`remember`)

**Status:** `[ ]` not started
**Owns:** the agent's durable, queryable memory beyond the current transcript
**Depends on:** [`README.md`](README.md), [`agent-state-and-compaction.md`](agent-state-and-compaction.md)

## Problem

Compaction preserves a summary of *this* conversation; memory preserves facts the agent should recall across conversations ("the project's deploy target", "the user prefers X"). The agent needs a way to store and **search** these.

## Model

- **`remember`** = an explicit tool the agent calls to persist a durable note (scoped to the agent within the workspace).
- **Storage** is workspace-scoped, keyed by agent, searchable (embedding + MMR + SQLite, mirroring `xai-grok-memory` in [`harness-reuse-map.md`](harness-reuse-map.md)).
- **Retrieval** is injected into context at the right time (session start, or ad-hoc when the agent queries).

## Data model

```rust
struct MemoryNote {
    id, agent_id, workspace_id,
    text,            // the durable fact
    embedding: Option<Vec<f32>>,
    created_at, updated_at,
    tags: Vec<String>,
}
```

## Boundaries

- Memory is **not** the same as compaction. Compaction preserves *this* conversation's arc; memory is cross-conversational and explicitly recalled.
- Secrets are never stored as memory notes / never returned in plain text (see [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).

## Tasks

- [ ] Add a `MemoryNote` store with embedding-based search.
- [ ] Expose `remember` + a query tool to agents.
- [ ] Wire retrieval into context assembly.
