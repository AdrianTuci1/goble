# 06 — Custom form components (overlay + backdrop)

**Status:** `[~]` — overlay (`Dialog`: centered panel + dimmed backdrop) + the model-provider form built; `FormField`/`SecretField`/`Toggle` primitives still pending
**Owns:** structured-input widgets (select, text field, toggle, secret field) rendered in an **overlay with a backdrop**
**Depends on:** [`README.md`](README.md), [`renderer-architecture.md`](renderer-architecture.md)

## Problem

We have to bind config-workflows — most prominently **connecting any type of model/API endpoint** — and the agent also needs to ask the user structured questions mid-conversation. We have no form primitives yet (no select/input/toggle components in the native shell). They should render as an **overlay (modal) with a backdrop**, consistent with the app's own look.

## Where it's used

| Case | What the form holds |
| --- | --- |
| **API endpoint / provider connector** | provider (select), api key / base URL / model, `api_key_secret_id` binding |
| **Remote workspace bootstrap** | secrets entry (custom composer) — see [`../02-first-run-and-routing/remote-bootstrap.md`](../02-first-run-and-routing/remote-bootstrap.md) |
| **Agent-driven forms** | a skill/plugin requests structured input (select/input/confirm) with a backdrop |

## Design

- **Overlay primitives**: `Overlay` (backdrop + centered panel), `FormField`, `Select`, `TextField`, `SecretField` (masked, bound to the vault), `Toggle`, `Submit`/`Cancel`. Must support keyboard + pointer, focus ring from the design system (see [`../10-platform-and-performance/README.md`](../10-platform-and-performance/README.md)).
- **Backdrop dims + separates**, matching the app's theme; the panel is a normal element tree so it picks up hover/selected states.
- **Interaction/direction** is taken from `~/Projects/warp-new` (see [`README.md`](README.md)); we build the widgets on `goble-ui`, not from a library.
- **Provider catalog**: the set of supported API endpoints/providers (OpenAI, Anthropic, OpenRouter, OpenAI-compatible, etc.). Port the whole "connect any API endpoint" logic from grok-build's model/config providers — reuse, not reimplement (see [`../04-agent-runtime/harness-reuse-map.md`](../04-agent-runtime/harness-reuse-map.md)).

## Tasks

- [~] Add overlay + form primitives to `goble-ui` — `Dialog` (backdrop + centered panel, click-outside close, unit-tested) is in; `FormField`/`SecretField`/`Toggle` still pending.
- [ ] Port the provider/API-endpoint connector logic from grok-build model config.
- [~] Build the provider-settings overlay form (`goble-ui-hot`/`model_form`) done; remote-bootstrap custom composer still pending.
- [ ] Let skills/plugins request structured input through the overlay.
