# Thread UI polish checklist: Slack-style sidebar and group chat

## `ChatMessage` model (`crates/goble-ui/src/elements/chat_content.rs`)

- [x] Add optional `author_name: Option<String>` to `ChatMessage`.
- [x] Add optional `timestamp: Option<String>` to `ChatMessage`.
- [x] Add builder methods `with_author_name` and `with_timestamp`.
- [x] Update `ChatMessage::from_thread_message` to populate author_name/timestamp when data is available.

## Group chat message elements

- [x] Create `crates/goble-ui/src/elements/group_chat_message.rs`.
  - [x] Renders a single message row: avatar column + content column.
  - [x] Content column shows author name, timestamp, and message fragments.
  - [x] Fragments are rendered in a compact inline-like style (text, bold, italic, code inline, links as chips, etc.).
- [x] Create `crates/goble-ui/src/elements/group_chat_message_group.rs`.
  - [x] Takes a slice of `ChatMessage` from the same author.
  - [x] First message shows avatar, author name, and timestamp.
  - [x] Subsequent messages show only content, indented under the author header.
- [x] Export new elements from `crates/goble-ui/src/elements.rs`.

## Thread sidebar

- [x] Create `crates/goble-ui/src/views/thread_sidebar.rs`.
  - [x] Sections: Channels, Direct Messages, Chats.
  - [x] Each section has a header with a count and a collapse/expand toggle.
  - [x] Items show kind icon, title, unread badge.
  - [x] Header "Threads" with a "+" new-thread button.
  - [x] `on_select`, `on_new`, and optional collapse callbacks.
- [x] Export `ThreadSidebar` from `crates/goble-ui/src/views/mod.rs`.

## Update `ThreadView`

- [x] Switch to group-chat layout: list of `GroupChatMessageGroup` instead of individual bubbles.
- [x] Group consecutive messages by `author_name` (fallback to `role`).
- [x] Keep composer below messages.
- [x] Keep empty state.

## Update `ThreadsContainer`

- [x] Replace `ThreadListView` with `ThreadSidebar`.
- [x] Pass sectioned threads to the sidebar.
- [x] Keep `ThreadView` on the right.

## Tests

- [x] `ChatMessage` builder tests for author_name and timestamp.
- [x] `GroupChatMessage` layout/paint test.
- [x] `GroupChatMessageGroup` grouping test.
- [x] `ThreadSidebar` section rendering and callback tests.
- [x] Updated `ThreadsContainer` layout test.

## Validation

- [x] `cargo test -p goble-ui` passes.
- [x] `cargo check --workspace --all-targets` passes.
- [x] `cargo test -p goble-desktop-service` passes.
- [x] `cargo check` in `crates/goble-desktop/src-tauri` passes.
- [x] `npm run build` in `crates/goble-desktop` passes.
