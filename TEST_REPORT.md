# Goble Test Report

Generated: 2026-07-26

## Summary

All workspace tests pass.

| Crate | Tests |
|-------|-------|
| goble-core (lib) | 107 passed |
| goble-core (integration) | 11 passed |
| goble-cli (lib) | 4 passed |
| goble-cli (integration) | 3 passed |
| goblin-worker (lib) | 11 passed |
| goblin-worker (bin) | 15 passed |
| goble-desktop (frontend) | 3 passed |
| **Total** | **154+ passed, 0 failed** |

## Recent additions

- MCP 4-level backend: search, install/list/update/delete, discover, execute + fallback.
- MCP desktop UI with side drawer for vault secret selection and enabled tool toggles.
- Tauri commands and API bindings for all MCP operations.
- `enabled_tools` filtering in tool definitions sent to the LLM.
- Encrypted credential vault.
- mTLS WebSocket handshake.
- Persistent scheduled task store with cron, heartbeat, manual, and HTTP triggers.

## Verification commands

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace

cd /root/goble/crates/goble-desktop
npm test
npm run build
npm run tauri build
```

All commands succeeded.

## Native UI (wgpu) baseline — 2026-08-24

Generated during Faza 0 of the Tauri → native migration on `feature/agent-guide-ui`.

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 100 passed |
| goble-desktop-service (lib) | 23 passed |
| goble-desktop-native (check) | compiles clean |
| goble-app (check) | compiles clean |
| workspace `cargo check --all-targets` | passes |

Fix included in baseline: `crates/goble-ui/src/elements/sheet.rs` — `Rect::contains` now receives a `PointF` (was passing a `Vector2F`), matching the pattern used by other input elements.

## Native UI (wgpu) — Faza 1 main view — 2026-08-24

Generated after Faza 1 of the Tauri → native migration on `feature/agent-guide-ui`: main view 100% built in `app/`.

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 101 passed (incl. new Stack event dispatch test) |
| goble-desktop-service (lib) | 23 passed |
| goble-app (build) | links clean |
| workspace `cargo check --workspace` | passes |

Delivered in this phase:
- 3-zone shell in `crates/goble-ui-hot`: main topbar (threads left; inbox + user settings right, buttons only), conversation sidebar (search + create + Plugins footer), terminal/chat (agent header, messages, composer).
- Terminal topbar cron button (right) opens the agent's crons drawer — `Sheet` overlay with scheduled-task list (create/delete/trigger) from real workflows.
- `Stack` now dispatches events topmost-first so the `Sheet` overlay receives clicks (backdrop close + panel interactions).
- Real-data integration: `goble-app` opens `DesktopState` with a `CollectingEventBus`; conversations, messages, crons (workflows) and agent name come from the backend; actions persist through `DesktopState` (create chat, send message, create/delete workflow); backend events (`chats:updated`, `chat:updated`, `workflows:updated`, `agents:updated`) are polled each frame and refresh the tree.
- Fallback to mock data when the store cannot be opened, so the shell stays runnable during development.

Verification: `cargo check --workspace`, `cargo test -p goble-ui -p goble-desktop-service`, `cargo build -p goble-app` — all pass.

## Native UI (wgpu) — Faza 2 vault + MCP connectors — 2026-08-24

Generated after Faza 2 of the Tauri → native migration on `feature/agent-guide-ui`: vault + MCP connectors panels built in `app/`, tied to real `DesktopState`.

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 101 passed |
| goble-desktop-service (lib) | 23 passed (incl. new `delete_vault_secret`) |
| goble-app (build) | links clean |
| goble-app (build --features hot-reload) | links clean |
| workspace `cargo check --workspace` | passes |

Delivered in this phase:
- `crates/goble-ui-hot` split into modules: `shell.rs`, `sidebar.rs`, `chat.rs`, `crons.rs`, plus new `connectors.rs` (MCP servers list + install/edit drawer) and `vault.rs` (passphrase unlock + secrets CRUD). New shared types: `VaultSecretEntry`, `McpServerEntry`, `McpSearchEntry`, `AiSnapshot`, `AiActions`.
- Vault panel: unlock with passphrase (real `unlock_vault`), list secrets, add/delete secrets; `delete_vault_secret` added to `DesktopState` (persists `vault_blob`, emits `vault:updated`).
- Connectors panel: searchable MCP server list with enable/disable switch, capability chips, discover/edit/delete actions; install drawer with registry search (`ConnectorCard`), manual source form (`npm`/`github`/`local`/`url`), and vault-secret selection; install/update/delete go through `DesktopState` MCP APIs.
- New `app/src/ai/` domain module: `AiState` mirrors the UI snapshot, refreshes from `DesktopState`; `make_ai_actions` wires all callbacks; mock fallback keeps the shell runnable without a backend.
- `main.rs` keeps a tokio runtime entered for the app lifetime (required by MCP search/install/update); `root_view.rs` drains `vault:updated` and rebuilds the `AiSnapshot`/`AiActions` each frame.
- Hot reload: `build_ui(app, snapshot, actions, ai_snapshot, ai_actions)` remains the single hot entry; removed stale `lib.rs.bak`.

Verification: `cargo check --workspace`, `cargo test -p goble-ui -p goble-desktop-service`, `cargo build -p goble-app`, `cargo build -p goble-app --features hot-reload` — all pass.

## Native UI (wgpu) — Faza 3 rich input tooltips + model menus — 2026-08-25

Generated during Faza 3 of the Tauri → native migration on `feature/agent-guide-ui`:
warp-style rich input buttons (hover tooltips + special menus).

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 112 passed (incl. Stack overlay + Tooltip + PopupMenu tests) |
| goble-desktop-service (lib) | 23 passed |
| goble-app (build) | links clean |
| goble-app (build --features hot-reload) | links clean |
| workspace `cargo check --all-targets` | passes |

Delivered in this phase:
- **A1 – `Stack` positioned overlays**: `with_overlay(child, offset)` layers an on-top
  child that does not grow the stack; overlays are laid out, painted last, and get the
  first chance to handle pointer events. Test `overlay_dispatches_before_children`.
- **A2 – `Tooltip`**: wraps any child and paints a compact message box (SurfaceRaised,
  radius 4) on hover, using render-time `PaintContext::hovered` so visibility survives
  the per-frame rebuild. `TooltipPosition::Above | Below`. Covers paint-on-hover + layout.
- **A3 – Composer button tooltips**: rich-input `+`, model, account and stop buttons now
  show Above tooltips ("Attach" / "Select model" / "Account" / "Stop").
- **A4 – model + account menus**: new `PopupMenu` — a trigger that opens a floating item
  menu using the A1 overlay pattern; open state is app-owned (`Rc<RefCell<bool>>`) so it
  survives rebuilds; selecting an item fires the select callback and closes.
  `PopupMenuItem` supports an optional leading icon, selected and disabled states. The
  model button opens a model list and the account button opens a Settings / Log out menu,
  driven by `UiSnapshot` (`models`, `selected_model`, `model_menu_open`,
  `profile_menu_open`) and the new `UiActions::on_model_select`. The plain-button path
  is kept as a fallback when no menu items are supplied.

Verification: `cargo check --workspace --all-targets`, `cargo test -p goble-ui -p
goble-desktop-service`, `cargo build -p goble-app`, `cargo build -p goble-app --features
hot-reload` — all pass.

## Native UI (wgpu) — Faza 4 chat renderer (block model) — 2026-08-25

Generated during Faza 4 of the Tauri → native migration on `feature/agent-guide-ui`:
the message transcript now renders through a proper block + inline model instead of
stacking every fragment as its own full-width row.

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 117 passed (incl. block grouping + InlineText tests) |
| goble-app (build) | links clean |
| workspace `cargo check --all-targets` | passes |

Delivered in this phase:
- **Block model** (`chat_content.rs`): new `ChatBlock` enum — `Paragraph(Vec<InlineSpan>)`
  plus dedicated `Heading` / `CodeBlock` / `List` / `BlockQuote` / `Action` / `Terminal`.
  `group_fragments_into_blocks` folds consecutive inline fragments (text/bold/italic/
  bold-italic/inline-code) into one paragraph, and keeps links + actions as interactive
  blocks so they stay clickable.
- **`InlineText` renderer** (`inline_text.rs`): a new element that lays out a sequence of
  styled `TextSpan`s as a single wrapping flow (bold/italic/mono/colors), so styled text
  inside one sentence flows inline instead of breaking onto its own row. Uses
  `measure_text_family` per span for word-level wrap; line-height-aware. Wrapping verified
  by a test (narrow width → taller output).
- **`ChatMessageBubble`** now renders blocks vertically, using `InlineText` for paragraphs
  and the existing widgets (code container, list, quote, heading, terminal, chip) for the
  rest. Bubble container, per-role alignment and max-width behaviour are unchanged.

Verification: `cargo test -p goble-ui`, `cargo check --workspace --all-targets`,
`cargo build -p goble-app` — all pass.

## Native UI (wgpu) — Faza 5 right chat-sidebar — 2026-08-25

Generated during Faza 5 of the Tauri → native migration on `feature/agent-guide-ui`:
the right chat-sidebar (Computer Use preview + Routines) is now wired into the
live chat view and toggled from the chat header.

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 119 passed (incl. sidebar rendering + ChatLayout composite tests) |
| goble-app (integration) | 19 passed (incl. new `toggle_right_sidebar_action_flips_state`) |
| workspace `cargo check --workspace --all-targets` | passes |

Delivered in this phase:
- **Header toggle**: the agent chat header gains a panel button (`left-panel-open` /
  `left-panel-close`, reflecting the current state) that shows/hides the right
  chat-sidebar. New app-owned `right_sidebar_open` on `UiState` / `UiSnapshot`, and a
  new `UiActions::on_toggle_right_sidebar` mutation.
- **Sidebar wiring**: when open, `build_agent_chat` wraps the `ChatView` in a
  `ChatLayout` with a `ChatSidebar` on the right. Hidden by default, per the design spec.
- **Routines from real data**: `ChatSidebar` now takes a `Vec<RoutineItem>` (title +
  schedule + enabled) derived from the agent's scheduled tasks (`state.crons`); a plus
  button in the Routines header opens the crons drawer so the user can add a task.
  Computer Use preview section is retained. `CHAT_SIDEBAR_WIDTH` and
  `CHAT_RIGHT_SIDEBAR_WIDTH` aligned to the 280px spec.
- **Verification**: `cargo test -p goble-ui -p goble-desktop-service -p goble-app`,
  `cargo check --workspace --all-targets` — all pass. Live window interaction was not
  exercised (native wgpu app, no browser automation); behavior is verified through the
  headless render tests and the app integration suite.

## Native UI (wgpu) — Faza 6 settings view — 2026-08-25

Generated during Faza 6 of the Tauri → native migration on `feature/agent-guide-ui`:
the Settings view is now wired into the live app with real app-owned state/actions
and a Back button (previously it was a hardcoded preview).

| Crate | Tests |
|-------|-------|
| goble-ui (lib) | 121 passed (incl. Back-button render + callback tests) |
| goble-app (integration) | 21 passed (incl. `settings_navigate_and_back_flip_state`, `toggle_dark_mode_updates_state`) |
| workspace `cargo check --workspace --all-targets` | passes |

Delivered in this phase:
- **Back button**: `SettingsView` gains a top-left Back button (chevron + label)
  via `with_on_back`, returning to the chat view.
- **App-owned settings state** (`UiState` / `UiSnapshot`): current `settings_page`,
  profile (name/email), dark-mode toggle, LLM config, workers, cluster + authorized
  keys lists, and vault-unlocked flag. Navigation and control changes survive the
  per-frame element rebuild.
- **Wired actions** (`UiActions`): `on_settings_back`, `on_settings_navigate`,
  `on_toggle_dark_mode`, `on_save_profile`, `on_save_llm` (persists via
  `DesktopState::set_llm_setting`), `on_add_worker` / `on_remove_worker`
  (persist via `add_worker` / `remove_worker`), `on_vault_unlock`, `on_create_cluster`
  / `on_unlock_cluster` (real cluster APIs), and app-owned authorized-key add/remove.
- **Live shell wiring**: the Settings tab builds `SettingsView` from the snapshot with
  all data + callbacks; pages render their controls (Appearance→Switch, LLM→Select +
  inputs, Profile/Workers/Keys→TextInput/Button). The sidebar lists 7 pages
  (Profile, LLM, Appearance, Account, Cluster, Workers, Keys).

Verification: `cargo test -p goble-ui -p goble-desktop-service -p goble-app`,
`cargo check --workspace --all-targets` — all pass. Live window interaction was not
exercised (native wgpu app, no browser automation); verified via headless render
tests + the app integration suite.

