# 04 — Deep research

**Status:** `[ ]` not started
**Owns:** long-running, multi-step research as a first-class routine
**Depends on:** [`README.md`](README.md), [`subagents.md`](subagents.md)

## Problem

Some tasks require sustained research: search, read, synthesize, iterate. This should be a **first-class routine** (like a workflow/cron) that the agent can kick off and that the user can watch, not ad-hoc prompting.

## Model

- Deep-research is a **routine**: a multi-step loop (plan → gather → synthesize → refine) with a bounded budget and explicit sources.
- It runs as a **sub-agent** (so it doesn't block the main chat) and emits progress ("reading source 3/8…") to the renderer.
- The result is a synthesized report plus the list of sources; it can be saved to the workspace (memory) for reuse.

## Reuse

- Built on the harness loop + tools (search/web) + sub-agents. Reuse the relevant tool/sampling crates from [`harness-reuse-map.md`](harness-reuse-map.md) (`xai-grok-tools`, `xai-grok-sampler`).

## Tasks

- [ ] Add a `DeepResearch` routine (steps, budget, progress events).
- [ ] Render progress + final report in the UI.
- [ ] Allow saving a research result to workspace memory.
