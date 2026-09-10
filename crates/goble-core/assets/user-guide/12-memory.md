# Memory

Goble agents can remember across turns. Memory is durable state stored per agent — distinct from the compacting transcript — so an agent keeps its goals, decisions, constraints and progress even when the conversation is summarized.

---

## The `MemoryNote` Store

A memory note is a short record in a dedicated `MemoryNote` store with **embedding-based search**. You can query past notes semantically rather than by exact text.

## How the Agent Uses It

- `memory_write` — record a goal, a decision, a constraint, or progress.
- `memory_read` / a query tool — look back at what an agent already knows.
- Retrieval is wired into **context assembly**, so relevant memories resurface when the agent needs them.

## What Survives

Memory survives conversation summarization. State (which agent, which workspace, what it's done) is keyed by `agent_id`, so it also survives local↔remote routing.

---

## Related

- [Agents](08-agents.md) — per-agent state.
- [Configuration](16-configuration.md) — where memory settings live.
