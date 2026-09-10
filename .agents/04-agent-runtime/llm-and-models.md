# 04 — LLM & model resolution

**Status:** `[~]` — `LlmProvider`/`llm` in `goble-core`; `DesktopState::run_chat_turn` now drives the harness loop (`Harness::run_turn`) on the chat send path, so tool calls execute and their output persists to the chat (deterministic `MockProvider` in tests); reasoning is still off by default and delta-level streaming to the renderer is pending
**Owns:** provider/model selection and the sampling loop
**Depends on:** [`README.md`](README.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)

## Problem

Agents run on configurable models/providers. The app resolves a provider's key from the workspace vault, picks a model, and streams a completion; the sampling loop turns prompt+context into a response and handles tool calls.

## Model resolution

- **Providers** are configured in the workspace TOML (`ProviderConfig { name, api_key_secret_id, base_url, model }`).
- Resolve → look up `api_key_secret_id` in the vault → build a provider client (`goble-core::llm::LlmProvider`).
- **Model** selection defaults from the TOML; the composer's model selector overrides per-conversation.

## Reuse

- `xai-grok-sampler` (the loop), `xai-grok-sampling-types`, `xai-grok-models` are the reference — see [`harness-reuse-map.md`](harness-reuse-map.md).
- `goble-core::llm` already has a `LlmProvider` trait + `MockProvider`, `resolve_llm_provider`.

## Streaming to the renderer

Token deltas + assistant deltas flow to both the renderer (see [`../06-renderer/renderer-architecture.md`](../06-renderer/renderer-architecture.md)) and the trace (see [`../07-observability/executions-and-trace.md`](../07-observability/executions-and-trace.md)).

## Tasks

- [x] Reuse `goble-core::llm` + grok-build sampler for the loop — `DesktopState::run_chat_turn` drives `Harness::run_turn` (the `run_mission_turn` sampler), so the send path runs the loop, executes tool calls and persists their output; compaction is still pending.
- [x] Wire the composer's model selector to actually change the model used — the dropdown is populated from the provider's model catalog (`goble-core::llm::provider_models`) and the selected model drives `run_chat_turn`, so picking a model really changes the one used.
- [~] Stream deltas to the renderer and trace in real time — the harness now streams assistant deltas into a single chat message row (so the renderer shows the reply progressively) and emits `chat:updated` per event; reasoning deltas and per-token trace emission are still pending.
