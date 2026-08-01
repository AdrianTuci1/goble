# Goble Tauri UI Migration — Agent Guide

## Objective
Replicate the standalone `agent-guide` demo UI (HTML/CSS/JS) 1:1 inside the existing Tauri React app at `goble/crates/goble-desktop` while preserving the existing Tauri command integrations and store model.

## Source
- `git@github.com:AdrianTuci1/agent-guide.git` cloned to `/root/agent-guide`
- Demo entry: `/root/agent-guide/demo/index.html` + `app.js` + `base.css` + `mock-data.js`
- Key view directories:
  - `demo/view/sidebar/`
  - `demo/view/agents-view/`
  - `demo/view/threads-view/`
  - `demo/view/right-sidebar/`
  - `demo/view/chat-window/chat/`
  - `demo/view/chat-window/composer/`
  - `demo/settings/settings-sidebar/`
  - `demo/settings/settings-content/`
- Utilities: `demo/workspace/identity-manager.js`, `demo/settings/design-system/design-system.js`, `demo/settings/settings-icons/settings-icons.js`, `demo/agents/agents-list/*`

## Target
- `goble/crates/goble-desktop/src/App.tsx` (shell)
- `goble/crates/goble-desktop/src/components/` (sidebar, chat area, right panel, agents view, title bar)
- `goble/crates/goble-desktop/src/pages/` (settings, connectors, agents)
- `goble/crates/goble-desktop/src/stores/appStore.ts` (extend state for new UI if needed)
- `goble/crates/goble-desktop/src/index.css` (global design tokens, scrollbars, inputs)

## Strategy
- Copy visual layout, component structure, and CSS variables from the demo, then wire existing Tauri API calls and Zustand store into the new UI.
- Keep the demo's dark color palette and flat look; avoid changing the language of the demo.
- Do not translate UI labels; keep English (as in the demo).
- Keep the Tauri API wrappers in `tauri/api.ts` intact; adapt UI components to call them.
- Preserve existing functional features (chat, workers, agents, MCP, vault, LLM settings) by reusing existing hooks and Tauri commands.

## Tracking

| # | Area | Source file(s) | Target file(s) | Status |
|---|------|----------------|----------------|--------|
| 1 | Global design tokens & scrollbars | `demo/base.css` | `src/index.css` | pending |
| 2 | App shell / title bar | `demo/index.html` layout, `app.js` | `App.tsx`, `TitleBar` | pending |
| 3 | Left sidebar | `demo/view/sidebar/*` | `components/Sidebar.tsx` + css | pending |
| 4 | Chat window layout | `demo/view/chat-window/*` | `components/ChatArea.tsx` + css | pending |
| 5 | Chat message rendering | `demo/view/chat-window/chat/*` | `components/ChatArea.tsx` | pending |
| 6 | Composer | `demo/view/chat-window/composer/*` | `components/ChatArea.tsx` | pending |
| 7 | Right sidebar (info/history) | `demo/view/right-sidebar/*` | new `components/RightSidebar.tsx` | pending |
| 8 | Agents view | `demo/view/agents-view/*`, `demo/agents/agents-list/*` | `pages/AgentsPage.tsx` | pending |
| 9 | Threads view | `demo/view/threads-view/*` | new `pages/ThreadsPage.tsx` (or merged into chat) | pending |
| 10 | Settings sidebar | `demo/settings/settings-sidebar/*` | `pages/SettingsPage.tsx` | pending |
| 11 | Settings content | `demo/settings/settings-content/*` | `pages/SettingsPage.tsx` | pending |
| 12 | Settings icons | `demo/settings/settings-icons/*` | shared icon components | pending |
| 13 | Identity manager | `demo/workspace/identity-manager.js` | adapters in store or utils | pending |
| 14 | Design system integration | `demo/settings/design-system/*` | theme class toggling | pending |
| 15 | Store updates | existing `appStore.ts` | extend for new UI state | pending |
| 16 | Routing update | `App.tsx` | add routes for new pages | pending |
| 17 | Build & type check | `npm run build` / `tsc` | verify no errors | pending |
| 18 | Tests | `npm test` | run and fix | pending |
| 19 | Git branch & PR | `feature/agent-guide-ui` | open PR | pending |

## Notes
- All secrets found in source files must be replaced with `[REDACTED]`.
- Prefer CSS variables matching the demo (`--ds-*`, `--tv-*`) so later theme switching works.
- Use the existing LLM provider list from `LLM_PROVIDERS` in `tauri/api.ts` for settings.

## Branch
`feature/agent-guide-ui`
