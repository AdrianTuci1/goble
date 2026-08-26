# Configuration

Goble's agent-visible configuration lives in `~/.goble/config.toml`. It is deliberately kept separate from secrets — the agent can read and patch the TOML, but never the vault of stored credential values.

---

## The Config File

On first launch Goble seeds `config.toml` from defaults (creating a `GobleConfig`). The in-memory copy is loaded on startup and saved back when you change provider/theme settings.

```
~/.goble/config.toml   # version, llm providers, theme, web_search, and more
```

`GobleConfig` currently covers `version`, `llm` (providers/models) and `theme`. It is meant to grow to cover plugins, workflows, personas, memory and deep-research.

## Agent-Readable and Agent-Editable

The agent can **read** the config to learn how the workspace is set up, and it can **patch** the TOML when a task calls for it. It never touches the credential vault that backs secret references — see [Credentials](04-credentials.md).

## Settings Surface

- **Model** — the active model; the composer selector changes it for the next turn.
- **Provider** — the LLM endpoint and its API-key credential.
- **Theme / appearance** — persisted to config.
- **Web search** — advanced web search on/off.

---

## Related

- [Authentication](03-authentication.md) — model providers and keys.
- [Monitoring Usage](18-monitoring-usage.md) — telemetry and usage settings.
