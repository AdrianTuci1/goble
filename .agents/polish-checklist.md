# Polish checklist: markdown parser and threads UI

## Markdown parser (`crates/goble-ui/src/elements/markdown.rs`)

- [x] Fix `flush_text()` to clear the active `StyleFrame` buffer after emitting a fragment.
- [x] Fix `take_pending_text()` to clear the active `StyleFrame` buffer / pending text after taking it.
- [x] Preserve link URL across nested bold/italic so `[**label**](url)` produces a link fragment.
- [x] Add support for headings (H1–H6) mapped to a new `ChatFragmentKind::Heading`.
- [x] Distinguish fenced code blocks (`ChatFragmentKind::CodeBlock { lang, code }`) from inline code (`ChatFragmentKind::Code`).
- [x] Preserve code block language when available.
- [x] Improve blockquote handling so inline styles inside a quote are not stripped.
- [x] Improve `merge_adjacent_text()` to avoid injecting unwanted spaces.
- [x] Add parser tests for:
  - [x] link with bold label
  - [x] link with italic label
  - [x] list item containing bold/italic
  - [x] heading levels 1–3
  - [x] fenced code block with language
  - [x] blockquote with inline emphasis
  - [x] text with hard line break (no duplication)

## `ChatFragment` API (`crates/goble-ui/src/elements/chat_content.rs`)

- [x] Add `ChatFragmentKind::Heading { level: u8, text: String }`.
- [x] Add `ChatFragmentKind::CodeBlock { lang: Option<String>, code: String }`.
- [x] Add builder methods `ChatFragment::heading(level, text)` and `ChatFragment::code_block(lang, code)`.
- [x] Update `ChatMessageBubble` tests if needed.

## `ChatMessageBubble` rendering (`crates/goble-ui/src/elements/chat_message_bubble.rs`)

- [x] Render `Code` as inline-styled text (small monospaced background chip).
- [x] Render `CodeBlock` with a block background, padding, and optional language label.
- [x] Render `Heading` with size based on level and bold weight.
- [x] Keep existing behavior for text, bold, italic, bold-italic, link, list, blockquote, line break, and action.

## Threads UI

### `ThreadListView` (`crates/goble-ui/src/views/thread_list_view.rs`)
- [x] Use `ThreadKind` to provide a visual cue (color or icon hint) for channel/direct/chat.
- [x] Add a callback test for `on_select`.

### `ThreadView` (`crates/goble-ui/src/views/thread_view.rs`)
- [x] Add empty state when `messages` is empty.
- [x] Keep composer available even when there are no messages.

### `ThreadsContainer` (`crates/goble-ui/src/views/threads_container.rs`)
- [x] Add empty state when `threads` is empty.
- [x] Keep `selected_id` fallback behavior.

## Validation

- [x] `cargo test -p goble-ui` passes.
- [x] `cargo check --workspace --all-targets` passes.
- [x] `cargo test -p goble-desktop-service` still passes.
- [x] `cargo check` in `crates/goble-desktop/src-tauri` passes.
