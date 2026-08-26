# 02 — Onboarding: no model key configured

**Status:** `[x]` — the empty-key banner is a modal overlay (no canned reply) and the model-provider overlay dialog + workspace-type prompt flow is implemented and integration-tested.
**Owns:** the empty-key state in the chat/composer
**Depends on:** [`README.md`](README.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)

## Problem

A brand-new user opens the app and sends a first message. Nothing is configured yet. Sending a message would just fail or silently no-op, so the UI must surface the blocker and offer the fix.

## Behavior (target)

1. User sends the first message (or opens a fresh conversation).
2. No LLM API key exists for any provider → the assistant's reply is replaced by a banner:
   *"You don't have any key configured, please click here to configure a model key."*
3. Clicking the banner opens the **model-provider overlay dialog** — a centered modal with a dimmed backdrop, built on `goble-ui`'s `Dialog` element — over the chat, where the user pastes the key and picks a provider/model. Save closes the dialog (and persists); Cancel / backdrop click discards.
4. After a key is stored (in the workspace vault), the onboarding continues to the workspace-type prompt, and the dialog closes.

## Rules

- The key is stored **in the workspace vault**, referenced by `api_key_secret_id` — never inline in the chat or in plain config. See [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md).
- The banner is a state, not a hard error: configurable providers are listed (OpenAI, Anthropic, OpenRouter, …) via the existing `get_llm_setting`/`set_llm_setting` surface in `goble-desktop-service`.
- Once at least one key exists, the banner disappears and the flow moves on.

## Gaps vs current code

- Current `desktop.get_llm_setting("openai")` default is a mock provider; the shell should surface the *empty-key* state rather than a canned reply. `on_send_message` in `app/src/actions.rs` currently appends a hardcoded assistant reply — it needs to check for a configured key/LLM first.
- The model-provider dialog stores the key via `on_save_llm` → `DesktopState::set_llm_setting`, referencing the provider inline; the vault `api_key_secret_id` binding is still pending.

## Tasks

- [x] Show an "empty LLM key" banner as a modal overlay instead of the canned reply — the canned reply is no longer sent on the no-key path; `on_send_message` keeps the user message and surfaces the banner overlay.
- [x] Wire the banner click to open the model-provider overlay dialog (`goble-ui` `Dialog` + `goble-ui-hot` `model_form`).
- [x] After a key is saved, close the dialog and re-route to the workspace-type prompt (see `router-local-vs-remote.md`).
