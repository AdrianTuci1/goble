---
**STATUS: SUPERSEDED / PARTIALLY IMPLEMENTED**
This document described early planning. Many items have since been implemented or superseded by later plans. Keep for historical context, but do not use as a current source of truth.
---

# Goble Desktop Gap Plan

**Goal:** Close remaining functional gaps so the desktop app navigation, sidebar, agents, settings, and inbox are all usable end-to-end.

**Branch:** `feature/agent-guide-ui`

---

## 1 — Wire missing page routes

### Objective
Pages exist in `src/pages/` but are not linked from the router or sidebar. Users cannot reach Workflows, Teams, Vault, Executions, Knowledge, or Search.

### Files to touch
- `crates/goble-desktop/src/App.tsx`
- `crates/goble-desktop/src/components/Sidebar.tsx`
- `crates/goble-desktop/src/components/Sidebar.css`
- `crates/goble-desktop/src/pages/SearchPage.tsx`
- `crates/goble-desktop/src/pages/WorkflowsPage.tsx`
- `crates/goble-desktop/src/pages/TeamsPage.tsx`
- `crates/goble-desktop/src/pages/VaultPage.tsx`
- `crates/goble-desktop/src/pages/ExecutionsPage.tsx`
- `crates/goble-desktop/src/pages/KnowledgePage.tsx`

### Implementation
1. Add routes in `App.tsx` for:
   - `/workflows` → `WorkflowsPage`
   - `/teams` → `TeamsPage`
   - `/vault` → `VaultPage`
   - `/executions` → `ExecutionsPage`
   - `/knowledge` → `KnowledgePage`
   - `/search` → `SearchPage`
2. Add sidebar icons/entries for each route (collapsed and expanded view).
3. Make `SearchPage` searchable across conversations, threads, agents, and vault secrets (local search, no backend needed).
4. Ensure each page has a basic non-empty layout and loads its data from the store or backend commands.

### Verification
- Click every sidebar item; each route renders without crashing.
- `npm run build` passes.
- `npm run test` passes.

### Commit
`feat(nav): wire all page routes and sidebar entries`

---

## 2 — Sidebar uses real data and unread badges

### Objective
Sidebar currently uses `agentsData` mock and shows a static thread icon. It should show real agents and conversations from the store and an unread badge for Threads.

### Files to touch
- `crates/goble-desktop/src/components/Sidebar.tsx`
- `crates/goble-desktop/src/components/Sidebar.css`
- `crates/goble-desktop/src/stores/appStore.ts`

### Implementation
1. Replace `agentsData` usage with `agents` from the store.
2. Add `unreadThreadsCount` selector in `appStore` that sums unread messages across all threads.
3. Show the unread count on the Threads icon in both collapsed and expanded sidebar.
4. Highlight the active route in the sidebar (use `useLocation`).
5. Keep existing conversation list behavior.

### Verification
- Create a channel, post a message from another participant, see badge count on Threads icon.
- Adding/removing agents updates the agent list in the sidebar without reload.

### Commit
`feat(sidebar): real agents, active route highlight, unread badge`

---

## 3 — AgentsPage with real agents and edit

### Objective
`AgentsPage` reads from mock data. It should display real agents, allow edit/delete, and provide a button to create a new agent.

### Files to touch
- `crates/goble-desktop/src/pages/AgentsPage.tsx`
- `crates/goble-desktop/src/pages/AgentsPage.css`
- `crates/goble-desktop/src/tauri/api.ts` (if needed for `updateAgent`)
- `crates/goble-desktop/src-tauri/src/lib.rs` (if backend needs update command)
- `crates/goble-desktop/src-tauri/src/state.rs` (if backend needs update method)

### Implementation
1. Replace `agentsData` with `agents` from store.
2. Add inline edit or expand card for name, description, prompt, tools.
3. If backend lacks `update_agent`, add it (re-use `createAgent`/`deleteAgent` patterns).
4. Add "New agent" button that opens a create form inline or navigates to Settings > Agents.
5. Delete agent from `AgentsPage` should work like in Settings.

### Verification
- Create agent, see it in `AgentsPage`, edit prompt, refresh, change persists.
- Delete agent from `AgentsPage`, it disappears from sidebar too.
- `npm run test` passes.

### Commit
`feat(agents): real agent list, edit, and create from agents page`

---

## 4 — RightSidebar agent info panel

### Objective
Right sidebar currently shows placeholder when an agent is selected from `AgentsPage`. It should show agent details, prompt, tools, and recent executions.

### Files to touch
- `crates/goble-desktop/src/components/RightSidebar.tsx`
- `crates/goble-desktop/src/components/RightSidebar.css`

### Implementation
1. When `selectedAgentId` is set, render agent info panel:
   - Name, description, prompt (collapsible).
   - List of tools if available.
   - Button to start a chat with that agent.
   - Recent executions for that agent (filter from `executions` in store).
2. Keep existing conversation/execution detail panels.

### Verification
- Select an agent in `AgentsPage`, right sidebar shows its details.
- Click "Chat with agent" navigates to `/chat?agent=<id>`.

### Commit
`feat(right-sidebar): agent info and execution history panel`

---

## 5 — Settings tabs implementation

### Objective
Multiple Settings tabs are still placeholders. Implement the most useful ones.

### 5a — Keys
**Files:** `SettingsPage.tsx`, `Pages.css`, backend `lib.rs`/`state.rs` if needed.
- List authorized keys (read from backend if `get_authorized_keys` exists, else local store).
- Add/remove public key by PEM paste.
- Show fingerprint and copy button.

### 5b — Notifications
**Files:** `SettingsPage.tsx`.
- Toggle desktop/push notifications.
- Toggle sound on new message.
- Toggle mention notifications.
- Persist to `localStorage`.

### 5c — Local archive
**Files:** `SettingsPage.tsx`, backend `lib.rs`/`state.rs` if needed.
- Export all local data (threads, messages, profile, keys) to JSON.
- Import from JSON (merge or replace with confirmation).

### 5d — Mobile
**Files:** `SettingsPage.tsx`.
- Placeholder replaced with pairing instructions / QR placeholder is acceptable, but must show useful text.

### 5e — Updates
**Files:** `SettingsPage.tsx`.
- Show current version and a "Check for updates" button (mock or use Tauri updater if available).
- Display release notes placeholder.

### Verification
- Each implemented tab renders useful controls.
- Export/import JSON round-trips without data loss.

### Commit
`feat(settings): implement keys, notifications, archive, mobile, updates tabs`

---

## 6 — Inbox / notifications for mentions and unread activity

### Objective
Provide a unified inbox view for unread activity, @mentions, and direct messages.

### Files to touch
- `crates/goble-desktop/src/pages/ThreadsPage.tsx` (add an inbox tab/view inside threads)
- `crates/goble-desktop/src/pages/ThreadsPage.css`
- `crates/goble-desktop/src/stores/appStore.ts`

### Implementation
1. Add `inboxItems` computed from threads:
   - All messages where current user is mentioned (content includes `@user:<id>` or `@user:<name>`).
   - All threads with unread messages sorted by latest activity.
2. Render an "Inbox" tab at the top of `ThreadsPage` (next to channels/DMs) or as a separate list section.
3. Clicking an inbox item opens the relevant thread and marks it read.
4. Add a backend event `thread:mention` if needed, or derive client-side from messages.

### Verification
- Send a message that mentions the user; it appears in inbox.
- Unread channel message appears in inbox.
- Inbox count decrements when item is opened.

### Commit
`feat(threads): unified inbox for mentions and unread activity`

---

## 7 — End-to-end verification and final push

### Verification commands
```bash
# Frontend
cd crates/goble-desktop
npm run build
npm run test

# Backend
cd crates/goble-desktop/src-tauri
cargo check -p goble-desktop-tauri --all-targets
cargo test -p goble-desktop-tauri --lib

# Core
cargo test -p goble-core
```

### Final commit
`chore: verify and close UI gap plan`

### Push
Push all commits to `feature/agent-guide-ui`.

---

## Notes
- Keep one commit per section above.
- Avoid adding new placeholder UI; replace existing placeholders with working behavior.
- Do not add real credentials or secrets to code.
- Report real tool output for each verification step.


---

## Verification Results

Run on branch `feature/agent-guide-ui`:

- Frontend build: `npm run build` ✅
- Frontend tests: `npm run test` — 11 passed ✅
- Backend check: `cargo check -p goble-desktop-tauri --all-targets` ✅ no warnings
- Backend tests: `cargo test -p goble-desktop-tauri --lib` — 23 passed ✅
- Core tests: `cargo test -p goble-core` — 149 passed ✅

All commits pushed to `feature/agent-guide-ui`.
