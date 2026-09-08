# Goble × Penpot design source

This folder holds the Penpot-ready design source for Goble.

## Files

- `goble-tokens.json` — DTCG design tokens (colors, spacing, radius, typography, density, shadows). Import it into Penpot via the **Tokens** panel.

## Workflow

1. Import `goble-tokens.json` into a new Penpot file.
2. Build / adjust components and layouts visually in Penpot using those tokens.
3. Export the final token set or share the Penpot link so the code can be updated to match.

## Current source of truth (code)

Tokens mirror:

- `crates/goble-ui/src/theme.rs` (Rust native UI theme — current source of
  truth for the wgpu native app)
- `crates/goble-desktop/src/utils/designSystem.ts` (TypeScript design system
  for the Tauri shell; values currently differ from `theme.rs`)
- `crates/goble-desktop/src/index.css` (CSS custom properties)

## Status

Penpot MCP server is configured in `~/.grok/config.toml`. The Penpot UI/plugin
side must also be open and connected to a file for MCP write operations to
work.

Ready-to-use scripts for this project are in `docs/design/scripts/`:

- `upload-tokens.js` — uploads `goble-tokens.json` into a Penpot token set.
- `create-layout.js` — builds the `Goble Native App` main page (topbar,
  agent sidebar overlay, chat content, composer). The + button in the sidebar
  creates a new agent. Chat messages are formatted like an agentic TUI: the
  user's prompt is echoed with a `❯` prefix and the agent's reply is plain
  text — no avatar, author name, timestamp or bubble. The chat-history panel
  open state is created as a separate component (`History panel open`), and
  the routines sidebar is created as a separate component
  (`Chat sidebar open`), so neither is embedded in the layout.
- `create-settings-view.js` — builds the `Goble Settings` page. The LLM page
  shows a header with an "Add model" button and a list of model cards.
  The "Add model" overlay and the catalog of settings row types are created
  as separate components on the same page.
- `create-threads-view.js` — builds the `Goble Threads` page. The layout
  matches `threads_container.rs`: a topbar, a collapsible `ThreadSidebar`
  on the left (Channels, Direct messages, Chats), a vertical divider, and
  a `ThreadView` on the right. The thread header is a full-width flat bar
  (surface background, not a floating card) and the composer spans the full
  thread width; messages keep the Slack-style grouped format (avatar +
  author + timestamp).
- `create-chat-custom-renders.js` — adds the custom conversation renders as
  separate components on the `Goble Native App` page (outside the shell, at
  x=1500): `AskUserCard` (agent question + quick replies + credential fields),
  `TerminalBlock`, `ToolCall card`, `Queued prompt card`, and a
  `Credential composer` variant.

Run them via `penpot__execute_code` once Penpot is connected. The generic
`penpot` skill describes the Penpot workflow without project-specific code.
