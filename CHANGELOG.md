# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-07-26

### Added

- **MCP support**: full 4-level backend implementation for Model Context Protocol servers.
  - Search and install MCP servers from npm/GitHub/local/URL/stdio sources.
  - Discover tool definitions from a running MCP server.
  - Execute tools directly or through a generic `mcp_<server>_<tool>` fallback.
  - Persist per-server metadata: selected vault secrets and enabled tools.
  - Tauri commands for `install_mcp_server`, `update_mcp_server`, `update_mcp_server_meta`, `delete_mcp_server`, `discover_mcp_tools`, `list_mcp_servers`, `search_mcp_servers`.
- **Desktop UI**: React + Tailwind + Zustand pages for chat, connectors, agents, workflows, workers, and vault.
  - MCP Connectors page with side drawer for secret selection and tool enable/disable toggles.
- **Encrypted credential vault**: passphrase-protected vault for API keys and MCP secrets.
- **Autonomous worker**: `goblin` binary with axum server, scheduler, task store, and WebSocket/mTLS support.
- **CLI**: `goble-cli` utility for worker management and scheduled tasks.
- **Testing**: workspace unit/integration tests, plus desktop component tests.
- **Release build**: Tauri builds `.deb`, `.rpm`, and `.AppImage` bundles.

### Changed

- N/A

### Fixed

- N/A

## [Unreleased]

- Work in progress features will be listed here.
