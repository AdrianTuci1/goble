# Agent 09 — Threads, agent tags, replies and conversation subthreads

## Status

Not started. This document is the implementation plan. No code has been written yet.

## Context

The Goble desktop UI currently has two separate conversation surfaces that do not talk to each other:

1. **Chat** (`/chat`, `ChatArea.tsx`, `appStore.conversations`)
   - 1-to-1 conversation with the assistant.
   - Messages are `ChatMessage { id, role, content, created_at }`.
   - Stored and loaded from the Tauri backend via `listChats`, `createChat`, `onChatsUpdated`, `onChatUpdated`.
2. **Threads** (`/threads`, `ThreadsPage.tsx`, `mocks/threadsData.ts`)
   - Slack/Discord-style workspaces, channels, DMs.
   - Messages are local mock-only objects with author, reactions, tags, `replyTo`.
   - Agents appear only as mock authors (`Fizz`, `Honey`) and are not wired to real `AgentInfo`.
   - User settings (profile, authorized keys, private channels) are hard-coded inside the page.

There is also no concept of **agent tags inside a conversation** — an agent cannot be @-mentioned, assigned to a thread, or labelled as owning a reply.

This plan unifies chat and threads into one backend model, adds real agent integration, and implements reply threads.

---

## 1. Goals

1. Add a `Thread` domain model in `goble-core` and persist it on the Tauri backend.
2. Allow the user to add/link agents to a thread (channel, DM or chat) and tag them.
3. Allow **replies** — a message can be a reply to another message, forming a subthread.
4. Keep the existing chat UI working while we migrate it to the new model.
5. Make user settings (profile, keys, authorized private channels) real and editable in Settings.
6. Provide backend commands for the Tauri layer and update the React store.
7. Add tests: Rust roundtrip + Tauri command tests, React unit tests, and one Playwright E2E for "tag agent + reply".

---

## 2. Data model (Rust — `goble-core`)

Agents are first-class participants in any thread. They are treated exactly like users: they can be added to channels, DMs and chats, they can post messages, they can be replied to, and they appear in the participant list.

```rust
// crates/goble-core/src/thread.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::agent::AgentId;
use crate::principal::{ParticipantId, UserId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    Chat,       // current 1-to-1 assistant chat
    Channel,    // threads-page channel
    Direct,     // threads-page DM
}

/// A participant is either a local user or an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Participant {
    User(UserId),
    Agent(AgentId),
}

impl Participant {
    pub fn participant_id(&self) -> ParticipantId {
        match self {
            Participant::User(u) => ParticipantId(format!("user:{}", u.0)),
            Participant::Agent(a) => ParticipantId(format!("agent:{}", a.0)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    pub kind: ThreadKind,
    pub title: String,
    pub owner_id: UserId,
    pub participants: Vec<Participant>,   // users AND agents together
    pub tags: Vec<String>,                // thread-level tags
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadMessage {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub author: Participant,              // user or agent
    pub content: String,
    pub reply_to: Option<MessageId>,      // parent message -> subthread
    pub tags: Vec<String>,                // e.g. #feature, #release
    pub participant_mentions: Vec<ParticipantId>, // @user or @agent
    pub reactions: Vec<Reaction>,
    pub attachments: Vec<Attachment>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub participant_id: ParticipantId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub blob_ref: String,                 // path or content-addressable id
}
```

### Invariants

- A `Thread` of kind `Chat` is owned by the current user and has exactly one `Participant::User(owner)`.
- A `Thread` of kind `Channel` can have any number of `Participant::User` and `Participant::Agent`.
- A `Thread` of kind `Direct` has exactly two participants (both users, both agents, or one of each).
- `reply_to` must point to a message in the same thread.
- A participant cannot appear twice in the same thread.
- Deleting a parent message does **not** delete replies; `reply_to` becomes an unresolved orphan and renders as "deleted message".
- Only the thread owner can add/remove participants.

### Principal model update

```rust
// crates/goble-core/src/principal.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct ParticipantId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct UserId(pub String);
```

`ParticipantId` is used for reactions, mentions, and any place where we need a unified identity regardless of user/agent.
---

## 3. Store and persistence (Tauri backend)

Add a `ThreadStore` in `goble-desktop/src-tauri/src/store.rs` (or a new module if `store.rs` is becoming too large).

```rust
pub struct ThreadStore {
    threads: std::sync::Mutex<Vec<Thread>>,
    messages: std::sync::Mutex<Vec<ThreadMessage>>,
    path: std::path::PathBuf,
}
```

Methods:

- `new(path: PathBuf) -> Self`
- `list_threads() -> Vec<Thread>`
- `create_thread(kind, title, owner_id, participants, tags) -> Thread`
- `delete_thread(id) -> bool`
- `add_participant(thread_id, participant) -> Result<(), ThreadError>`
- `remove_participant(thread_id, participant_id) -> Result<(), ThreadError>`
- `list_participants(thread_id) -> Vec<Participant>`
- `list_messages(thread_id) -> Vec<ThreadMessage>`
- `post_message(thread_id, author, content, reply_to, tags, attachments) -> ThreadMessage`
- `add_reaction(message_id, participant_id, emoji) -> Result<(), ThreadError>`
- `remove_reaction(message_id, participant_id, emoji) -> Result<(), ThreadError>`
- `save() -> anyhow::Result<()>`
- `load() -> anyhow::Result<()>`

Persistence format: one JSONL file per thread for messages, one JSON file for thread metadata. Keep it simple; we can move to SQLite later when the scheduler store matures.

### Migration path from existing chats

On first startup after this change:

1. Load existing `chats.json` (or whatever the current backend uses for chat history).
2. For every existing chat, create a `Thread { kind: Chat, ... }`.
3. Convert existing `ChatMessage` rows into `ThreadMessage { author: User(owner), content, ... }`.
4. Write the new store files.
5. Keep a `migrated_version` marker so we never run the migration twice.

---

## 4. User settings

Current state: `currentUser` is hard-coded in `threadsData.ts`.

Add a real `UserProfile` model:

```rust
// crates/goble-core/src/user.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: UserId,
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub public_key_pem: Option<String>,
}
```

Backend:

- `UserStore` persisted next to `ThreadStore`.
- Tauri commands:
  - `get_user_profile() -> UserProfile`
  - `set_user_profile(profile) -> Result<(), UserError>`
  - `list_authorized_keys() -> Vec<AuthorizedKey>`
  - `add_authorized_key(...) -> Result<AuthorizedKey, UserError>`
  - `remove_authorized_key(id) -> bool`

Frontend:

- Add a "Profile" section in `SettingsPage.tsx`.
- Add an "Access" section for authorized keys and private-channel membership.
- The threads page reads the real profile instead of `mockCurrentUser`.

---

## 5. Tauri commands

Add these to `goble-desktop/src-tauri/src/lib.rs`:

```rust
#[tauri::command]
fn get_threads(state: State<AppState>) -> Vec<ThreadSummary> { ... }
```rust
#[tauri::command]
fn create_thread(
    kind: ThreadKind,
    title: String,
    participants: Vec<Participant>,     // users and/or agents
    tags: Vec<String>,
    state: State<AppState>,
) -> Result<Thread, ThreadError> { ... }

#[tauri::command]
fn delete_thread(id: String, state: State<AppState>) -> bool { ... }

#[tauri::command]
fn add_thread_participant(
    thread_id: String,
    participant: Participant,
    state: State<AppState>,
) -> Result<(), ThreadError> { ... }

#[tauri::command]
fn remove_thread_participant(
    thread_id: String,
    participant_id: ParticipantId,
    state: State<AppState>,
) -> Result<(), ThreadError> { ... }

#[tauri::command]
fn get_thread_participants(
    thread_id: String,
    state: State<AppState>,
) -> Vec<Participant> { ... }

#[tauri::command]
fn get_messages(thread_id: String, state: State<AppState>) -> Vec<ThreadMessage> { ... }

#[tauri::command]
fn post_message(
    thread_id: String,
    content: String,
    reply_to: Option<String>,
    tags: Vec<String>,
    state: State<AppState>,
) -> Result<ThreadMessage, ThreadError> { ... }

#[tauri::command]
fn add_reaction(
    message_id: String,
    emoji: String,
    state: State<AppState>,
) -> Result<(), ThreadError> { ... }

#[tauri::command]
fn remove_reaction(
    message_id: String,
    emoji: String,
    state: State<AppState>,
) -> Result<(), ThreadError> { ... }
```

Events to emit:

- `thread:created`, `thread:updated`, `thread:deleted`
- `thread:message`, `thread:message_updated`, `thread:message_deleted`
- `thread:reaction`

Frontend listens via `api.ts` helpers (`onThreadCreated`, `onThreadMessage`, etc.).

---

## 6. Frontend changes

### 6.1 Store (`appStore.ts`)

Add:

```ts
threads: ThreadSummary[];
activeThreadId: string | null;
threadMessages: Record<string, ThreadMessage[]>;
setThreads: (threads: ThreadSummary[]) => void;
addThread: (thread: ThreadSummary) => void;
setActiveThreadId: (id: string | null) => void;
setThreadMessages: (threadId: string, messages: ThreadMessage[]) => void;
addThreadMessage: (threadId: string, message: ThreadMessage) => void;
```

Keep old `conversations`/`messages` fields during migration, but the new code should target `threads`.

### 6.2 API layer (`tauri/api.ts`)

Add invoke wrappers for every command above and `listen` helpers for every event.

### 6.3 Threads page (`ThreadsPage.tsx`)

- Replace local mock state with store-backed state.
- Load workspaces/channels/DMs from real `Thread` objects.
- When posting a message, call `postMessage` instead of mutating local arrays.
- Keep the existing UI layout but render from `threadMessages`.
- Add "Add participant" dropdown in the channel/DM header:
  - lists users (from `UserStore`/contacts) **and** real agents (from `appStore.agents`),
  - on select calls `addThreadParticipant` with `Participant::User` or `Participant::Agent`,
  - agent messages are rendered with a 🤖 avatar, users with initials.
- Tagging participants in messages:
  - typing `@` shows a combined user + agent picker,
  - selected participant becomes a `participant_mention`,
  - the mentioned participant can be a user or an agent.
- Agent-as-participant behavior:
  - linked agents appear in the participant list exactly like users,
  - they can be replied to,
  - mentioning an agent triggers the harness exactly like `/run agent`.
- Add reply rendering:
  - when a message has `replyTo`, render a small "↳ replying to …" preview,
  - clicking a reply count opens the subthread in a side panel or inline.

### 6.4 Chat area (`ChatArea.tsx`)

- Migrate to the thread model:
  - a chat is a `Thread` of kind `Chat`,
  - messages are `ThreadMessage` with `author: Participant::User` or `author: Participant::Agent`,
  - the assistant role maps to the currently selected agent (or a default "Goble" agent).
- Support `@participant` mentions:
  - typing `@` shows a combined user + agent picker,
  - selected participant is stored in `participant_mentions`,
  - backend/harness receives the mention and can dispatch to that participant (agent) or notify a user.
- Support replying to any previous message, including agent messages.

### 6.5 Right sidebar (`RightSidebar.tsx`)

- For a selected thread, show:
  - participants (users + linked agents),
  - thread tags,
  - pinned messages (future — not in first pass),
  - reply statistics.
- For a selected agent, keep the existing agent panel.

---

## 7. Agent integration in threads

### 7.1 Adding/removing participants

- UI: channel/DM header → "Members" → list of users + agents → click to add.
- Backend: `Thread.participants.push(participant)` if not already present; emit `thread:updated`.
- Permission: only thread owner can add/remove participants.

### 7.2 Agent as participant

- An agent added to a thread is represented as `Participant::Agent(agent_id)`.
- It appears in the member list, can be @-mentioned, and can be the target of a reply.
- Removing an agent from a thread is identical to removing a user: `remove_participant(thread_id, participant_id)`.

### 7.3 Agent replies

- When a message contains `@<agent>` the backend invokes that agent via the harness.
- When a message contains `@<user>` the backend can notify/mention that user.
- For the first pass, agent replies are posted by the Tauri backend as `ThreadMessage { author: Participant::Agent(agent_id), ... }`.
- The harness path is reused: `run_agent` / `execute_tool_call` etc.
- The agent sees the thread history as context (with a budget; future compression).

### 7.4 Tags vs mentions

- **Tags** (`#bug`, `#feature`) are free-form labels on messages/threads.
- **Mentions** (`@participant`) explicitly notify or invoke a participant.
- **Participants** (users or agents) are visible in the thread member list.

---

## 8. Reply / subthread UX

### 8.1 Inline replies

- Every message has a "Reply" button.
- The composer enters reply mode (same as current `replyTo` state).
- Replies are rendered immediately below the parent in a collapsible group.

### 8.2 Subthread panel

- Clicking "N replies" opens a narrow side panel (right sidebar tab `replies`) that shows only the reply chain.
- Useful for channels with high traffic.

### 8.3 Data fetching

- `get_messages(thread_id)` returns all messages including replies.
- Frontend builds the parent/child tree in memory.

---

## 9. Settings integration

Add to `SettingsPage.tsx`:

1. **Profile** tab
   - name, email, avatar URL,
   - public key fingerprint,
   - save calls `set_user_profile`.
2. **Access** tab
   - list of authorized public keys,
   - add/remove,
   - private channels each key can access.
3. **Threads** tab (future)
   - default notification settings,
   - workspace-level tags.

---

## 10. Test plan

### Rust

- [ ] `crates/goble-core/tests/thread_roundtrip.rs`
   - serialize/deserialize `Thread` and `ThreadMessage`.
   - verify `Participant` roundtrip, duplicate-participant rejection, and `Direct` thread participant count.
2. `crates/goble-desktop/src-tauri/src/thread_store.rs` unit tests (or separate `tests/thread_store.rs`)
   - create thread, post message, reply, add/remove user participant, add/remove agent participant, reaction persistence.
3. Tauri command smoke tests using `tauri::test` if available; otherwise integration tests that call `ThreadStore` directly.

### TypeScript

1. `src/stores/appStore.threads.test.ts`
   - adding a thread, setting active thread, adding messages, replies.
2. `src/tests/ThreadsPage.test.tsx`
   - mocked Tauri: render page, switch channel, post message, reply to message, tag agent.

### E2E

1. `e2e/threads-agent-reply.spec.ts`
   - open threads page,
   - create a channel,
   - add a real user and an agent as participants,
   - post a message with `@agent`,
   - assert agent reply appears,
   - reply to the agent reply,
   - add a human participant and assert they appear in the member list.

---

## 11. Implementation order

Recommended sequence so the UI never stays broken for long:

1. Add `Thread`/`ThreadMessage` models in `goble-core` + roundtrip tests.
2. Add `ThreadStore` + `UserStore` on Tauri backend + persistence tests.
3. Add Tauri commands and events; keep them unused in UI for one commit.
4. Migrate `ChatArea.tsx` to the new model (chat is now a thread).
5. Migrate `ThreadsPage.tsx` from mock data to real store.
6. Add agent linking and `@agent` mentions.
7. Add reply rendering and subthread panel.
8. Add Settings profile/access tabs.
9. Add React unit tests.
10. Add Playwright E2E.

---

## 12. Open questions

1. Do we want Slack-style workspaces as a separate entity, or can a workspace just be a tag/filter on `Thread`?
   - **Proposal:** keep `Workspace` as a UI-side grouping for now; `Thread` has an optional `workspace_id`. This avoids over-engineering.
2. Should DMs be encrypted end-to-end, or are they local-only plaintext for the first pass?
   - **Proposal:** plaintext local storage first; encryption is a later milestone.
3. Do reactions need to be persisted immediately, or can they be optimistic + synced?
   - **Proposal:** persisted immediately via `add_reaction` command.
4. How does an agent "see" a thread history?
   - **Proposal:** the backend builds a truncated message list and passes it as `prompt` context to `run_agent`.

---

## 13. Files to touch

### Rust

- `crates/goble-core/src/lib.rs` — export new modules.
- `crates/goble-core/src/thread.rs` — new.
- `crates/goble-core/src/user.rs` — new or extend existing principal module.
- `crates/goble-core/src/error.rs` — add `ThreadError`, `UserError`.
- `crates/goble-desktop/src-tauri/src/store.rs` — add `ThreadStore`, `UserStore`.
- `crates/goble-desktop/src-tauri/src/lib.rs` — register commands and state.

### TypeScript

- `crates/goble-desktop/src/stores/appStore.ts` — add thread state.
- `crates/goble-desktop/src/tauri/api.ts` — add invoke/listen helpers.
- `crates/goble-desktop/src/pages/ThreadsPage.tsx` — wire to real data.
- `crates/goble-desktop/src/components/ChatArea.tsx` — migrate to thread model.
- `crates/goble-desktop/src/components/RightSidebar.tsx` — thread/agent panel.
- `crates/goble-desktop/src/pages/SettingsPage.tsx` — profile/access tabs.
- `crates/goble-desktop/src/mocks/threadsData.ts` — delete or keep as fallback seed only.

### Tests

- `crates/goble-core/tests/thread_roundtrip.rs`
- `crates/goble-desktop/src-tauri/src/thread_store.rs` (inline tests) or `crates/goble-desktop/src-tauri/tests/thread_store.rs`
- `crates/goble-desktop/src/stores/appStore.threads.test.ts`
- `crates/goble-desktop/src/tests/ThreadsPage.test.tsx`
- `crates/goble-desktop/e2e/threads-agent-reply.spec.ts`

---

## 14. Definition of done

- [ ] `Thread` and `ThreadMessage` models exist in `goble-core` with tests.
- [ ] Tauri backend persists threads and messages.
- [ ] Existing chat history is migrated to the new model on first startup.
- [ ] Threads page loads real data; mock-only fallback is removed.
- [ ] User can add/remove any participant (user or agent) to/from a thread.
- [ ] User can @mention any participant (user or agent) in a message.
- [ ] Mentioning an agent triggers the harness / agent reply.
- [ ] User can reply to any message, including agent messages.
- [ ] User profile and authorized keys are editable in Settings.
- [ ] Rust tests, `npm run build`, Vitest tests, and Playwright E2E pass.
- [ ] Update `TEST_REPORT.md` with new test counts.

