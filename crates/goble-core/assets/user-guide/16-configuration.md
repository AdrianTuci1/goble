# Configuration

Goble's settings live in `~/.goble/config.toml`. The layout is the one grok writes (`~/.grok/config.toml`), so a file written for either tool reads in the other: one `[model.<slug>]` table per model, the default selection under `[models]`, MCP servers under `[mcp_servers.<name>]`.

Every section and every key is optional. A file that sets a single key parses; the rest keeps its built-in default.

---

## The Config File

On first launch Goble seeds `config.toml` from defaults. The in-memory copy is loaded on startup and saved back when you change settings.

```
~/.goble/config.toml   # models, default model, MCP servers, theme
```

## Models

Each model is one table keyed by a slug of your choosing:

```toml
[model.deepseek]
model = "deepseek-flash"            # the id sent to the API
name = "Deepseek-V4-Flash"          # the label shown in the picker
base_url = "https://api.deepseek.com"
api_key = "sk-..."                  # the literal key, inline
max_completion_tokens = 16384
context_window = 256000

[models]
default = "deepseek"                # the `[model.<slug>]` key above
```

- `model` — the id sent to the API. Omit it and the slug is the id.
- `name` — the label in the composer's model tray. Omit it and the id is shown.
- `base_url` — the provider endpoint. An endpoint that isn't Anthropic, OpenRouter or Ollama is spoken to as OpenAI-compatible.
- `api_key` — the literal key for this model. A model with a key here is usable without any per-provider setting.
- `max_completion_tokens`, `context_window` — the response cap and the context window the model is sized for.
- `hidden = true` — keep the entry but leave it out of the picker; it stays usable by naming it.

`[models] default` names a slug. A default that names no table resolves to itself, which is how a built-in catalog model is pinned.

## MCP Servers

```toml
[mcp_servers.penpot]
url = "https://design.penpot.app/mcp/stream"
enabled = true

[mcp_servers.gh]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
```

## Theme

`[theme]` is Goble's own section — the reference layout has no equivalent, so it is kept here rather than dropped:

```toml
[theme]
dark = true
accent = "#14b8a6"
primary = "#e5e7eb"     # optional
secondary = "#9ca3af"   # optional
```

## Sections Goble Does Not Model

A section this schema does not know (grok's `[cli]`, `[ui]`, `[marketplace]`, `[privacy]`, …) is kept exactly as it was read and written back on the next save, so pointing Goble at a shared file does not lose another tool's settings.

## Loading, and Files That Do Not Parse

- A section that does not parse costs only that section: its built-in default applies, the reason is logged, and every other section is kept.
- A file that is not TOML at all is refused, logged, and the config already in memory is kept — a bad edit cannot leave the app with no models.
- A file in the older shape (`version` + `[llm]` + `[theme]`) still loads: its `[[llm.models]]` entries become `[model.<slug>]` tables, `llm.default_model` becomes `[models] default`, and its `api_key_secret_id` moves into the model's inline `api_key`. The migrated file is written on the next save, and never before.

## Agent-Readable and Agent-Editable

The agent can **read** the config to learn how the workspace is set up, and it can **patch** the TOML when a task calls for it.

Because model keys are now written inline, `config.toml` holds credentials in the clear: keep it out of version control and off shared machines. Goble's credential vault — not this file — backs the secrets the rest of the app resolves by id; see [Credentials](04-credentials.md).

## Settings Surface

- **Model** — the active model; the composer selector changes it for the next turn.
- **Provider** — the LLM endpoint and its API-key credential.
- **Theme / appearance** — persisted to config.
- **Web search** — advanced web search on/off.

---

## Related

- [Authentication](03-authentication.md) — model providers and keys.
- [Monitoring Usage](18-monitoring-usage.md) — telemetry and usage settings.
