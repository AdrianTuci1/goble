---
**STATUS: SUPERSEDED / PARTIALLY IMPLEMENTED**
This document described early planning. Many items have since been implemented or superseded by later plans. Keep for historical context, but do not use as a current source of truth.
---

# Goble UI Polish Roadmap

**Goal:** close functional gaps in Threads, Settings, Agent/Worker management, and polish execution traces so the desktop app is usable end-to-end.

**Branch:** `feature/agent-guide-ui`  
**Legend:** `[ ]` pending `[>]` in progress `[x]` done

---

## 1 — Real-time threads and unread state

**Objective:** Make the Threads page feel live: new messages appear without a full reload, and unread badges help users notice activity.

- **Files to touch:**
  - `crates/goble-desktop/src/pages/ThreadsPage.tsx`
  - `crates/goble-desktop/src/stores/appStore.ts`
  - `crates/goble-desktop/src-tauri/src/state.rs`
  - `crates/goble-desktop/src-tauri/src/lib.rs`
- **Backend changes:**
  1. Add Tauri event `thread:message` emitted when a message is posted to any thread.
  2. Add `thread:updated` event when thread metadata changes.
- **Frontend changes:**
  1. Extend `ThreadSummary` with `last_read_at` and `unread_count` fields.
  2. Update `ThreadsPage` to listen for `thread:message` events and append messages to the matching thread.
  3. Show unread badge on channel/DM/chat items.
  4. Mark thread as read when selected (`markThreadRead` command + store update).

**Verification:**
- Open two clients or send a message from another source; the receiving Threads page shows the new message and a badge.

**Commit:** `feat: real-time thread updates and unread badges`

---

## 2 — @mention autocomplete in composer

**Objective:** Let users mention agents and users with an autocomplete popup instead of memorizing `@agent:<id>` syntax.

- **Files to touch:**
  - `crates/goble-desktop/src/pages/ThreadsPage.tsx`
  - `crates/goble-desktop/src/pages/ThreadsPage.css`
- **Implementation:**
  1. Detect when the user types `@` followed by optional text in the composer.
  2. Show a popover with matching participants (users + agents) of the active thread.
  3. Insert the selected mention as `@agent:<id>` or `@user:<id>`.
  4. Keep `extractMentions` parsing compatible.

**Verification:**
- Type `@` in a thread composer with agents; select an agent; the sent message triggers the agent run.

**Commit:** `feat: @mention autocomplete in thread composer`

---

## 3 — Runtime feedback when an agent is running

**Objective:** Show the user that an agent mention was dispatched and is waiting on a worker.

- **Files to touch:**
  - `crates/goble-desktop/src/pages/ThreadsPage.tsx`
  - `crates/goble-desktop/src/stores/appStore.ts`
- **Implementation:**
  1. Add per-thread `pendingMentions` set in the store.
  2. When `runAgentForThreadReply` is called, add the mention id to `pendingMentions`.
  3. When a matching `ThreadAgentReply` or message arrives from that agent, remove it.
  4. Render a small spinner / “thinking” indicator next to the composer or on the pending message.

**Verification:**
- Mention an agent with no reachable worker: spinner appears briefly and a subtle error hint is shown.
- With a reachable worker: spinner disappears when the reply arrives.

**Commit:** `feat: pending agent runtime feedback in threads`

---

## 4 — Message edit/delete

**Objective:** Basic message moderation in threads.

- **Files to touch:**
  - `crates/goble-desktop/src/pages/ThreadsPage.tsx`
  - `crates/goble-desktop/src-tauri/src/state.rs`
  - `crates/goble-desktop/src-tauri/src/lib.rs`
  - `crates/goble-core/src/store.rs`
- **Backend changes:**
  1. `Store::update_thread_message(thread_id, message_id, content)`.
  2. `Store::delete_thread_message(thread_id, message_id)`.
  3. Tauri commands `update_thread_message` and `delete_thread_message`.
  4. Emit `thread:message:updated` / `thread:message:deleted` events.
- **Frontend changes:**
  1. Add hover actions on own messages: Edit and Delete.
  2. Inline edit mode in composer replaces content on save.
  3. Update store and re-render on events.

**Verification:**
- Edit a message, refresh page, change persists.
- Delete a message, it disappears and stays gone after refresh.

**Commit:** `feat: edit and delete thread messages`

---

## 5 — Persist appearance settings

**Objective:** Theme/accent/density/radius choices survive app restart.

- **Files to touch:**
  - `crates/goble-desktop/src/stores/appStore.ts`
  - `crates/goble-desktop/src/pages/SettingsPage.tsx`
- **Implementation:**
  1. Load design from `localStorage` on startup.
  2. Persist design to `localStorage` on every change.
  3. Apply design CSS variables to `document.body` via an effect in `App.tsx` or a hook.

**Verification:**
- Change theme to light, reload app, theme is still light.

**Commit:** `feat: persist appearance settings in localStorage`

---

## 6 — Flesh out Settings > Compute and Settings > Agents

**Objective:** Replace placeholders with useful UIs.

### Compute tab
- **Files to touch:**
  - `crates/goble-desktop/src/pages/SettingsPage.tsx`
- **Implementation:**
  1. Move the full `WorkerSettings` component from Settings into the `Compute` tab.
  2. Add cluster install card in Compute as a subsection.
  3. Add worker actions: remove worker, edit worker URL, show tags.

### Agents tab
- **Files to touch:**
  - `crates/goble-desktop/src/pages/SettingsPage.tsx`
  - `crates/goble-desktop/src/tauri/api.ts` if needed
- **Implementation:**
  1. List agents with name, description, tools.
  2. Allow quick create/delete from settings.
  3. Link to the existing agent builder page if one exists.

**Verification:**
- Navigate Settings → Compute and see workers + cluster install.
- Navigate Settings → Agents and see agent list + create/delete.

**Commit:** `feat: compute and agents settings tabs`

---

## 7 — Cleanup dead code and warnings

**Objective:** Reduce noise before further work.

- **Files to touch:**
  - `crates/goble-desktop/src-tauri/src/lib.rs` (remove unused `chat_id`)
  - `crates/goble-desktop/src-tauri/src/state.rs` (remove unused `migrate_legacy_chats_to_threads`, `get_authorized_keys` if truly unused)
  - `crates/goble-desktop/src/thread_store.rs` (unused `ThreadMessage` import)
- **Verification:**
  - `cargo check -p goble-desktop-tauri` with no warnings.
  - `npx tsc -b` clean.

**Commit:** `chore: remove dead code and compiler warnings`

---

## Cross-cutting verification commands

```bash
# Frontend
cd crates/goble-desktop && npx tsc -b

# Backend
cd crates/goble-desktop/src-tauri && cargo check && cargo test -p goble-desktop-tauri --lib

# Core
cargo test -p goble-core
```

---

## Notes

- Keep changes focused on one item per commit.
- For UI-only changes, prefer `feat:` if behavior is new, `chore:` for cleanup.
- Avoid adding new placeholder UI; replace existing placeholders with working behavior.
