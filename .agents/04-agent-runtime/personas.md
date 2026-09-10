# 04 — Personas

**Status:** `[ ]` not started
**Owns:** the reusable persona definitions that shape an agent's behavior/voice
**Depends on:** [`README.md`](README.md)

## Problem

An agent's behavior is defined by more than its tools — it has a **persona**: role, tone, priorities, constraints, and the style of its output. Personas should be reusable and configurable per agent.

## Model

```rust
struct Persona {
    id: PersonaId,
    name: String,            // e.g. "Reviewer", "Planner"
    role: String,            // what the persona is for
    system_prompt: String,   // the voice/behavior stem
    constraints: Vec<String>,
    temperature: Option<f32>,
    tags: Vec<String>,
}
```

- A persona is a **shared definition** in the workspace; an agent references a persona by id (see [`../03-workspace-model/multiple-agents.md`](../03-workspace-model/multiple-agents.md)).
- The harness **assembles the system prompt** from the persona + the agent's config + the workspace rules (reuse `xai-grok-agent` prompt assembly).
- Personas are **agent-editable like the TOML** — a "persona" is part of the workspace config the user (and the agent) can tune.

## Tasks

- [ ] Define the `Persona` config and store it in the workspace TOML.
- [ ] Assemble system prompts from persona + config.
- [ ] Let personas be created/edited from natural language.
