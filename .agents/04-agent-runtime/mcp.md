# 04 — MCP (dynamic tools)

**Status:** `[~]` partial: McpManager + install/discover exist in `goble-core`; runtime dynamic install pending
**Owns:** how agents discover, download and enable MCP servers at runtime
**Depends on:** [`README.md`](README.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)

## Problem

Part of the product promise is that an agent can **search and download MCP servers dynamically** ("I need a JSON-only search tool → install X"). The harness must be able to bring a server up (npm/github/local/url), discover its tools, and expose them.

## Existing surface (`goble-core`)

`McpManager`, `McpServer`, `McpSource::{Github,Npm,Local,Url}`, `McpRuntime::{V8Isolate,Binary}`, plus `McpRegistry` (search) and `McpInstaller`. The desktop service exposes `install_mcp_server`, `discover_mcp_tools`, `update_mcp_server_meta`, `search_mcp_servers`.

## Design (target)

- **Install** = resolve source, fetch, set up runtime, prepare credentials (`vault` secret refs / OAuth via `xai-grok-mcp`).
- **Discover** = connect + list tools → populate the tool registry.
- **Enable/disable** = which tools the agent may actually call (per workspace config).
- **Lifecycle** = start/stop server processes; stream stderr/logs to the renderer + traces.

## Rule

- MCP servers are **workspace-scoped** and their credentials come from the workspace vault, never inline config (see [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).

## Tasks

- [ ] Drive MCP install/discover from the harness (not only from a UI panel).
- [ ] Make an agent able to request an MCP install mid-conversation.
- [ ] Surface MCP server status + tool list in the renderer (connectors panel already exists).
