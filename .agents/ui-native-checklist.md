# UI Native Implementation Checklist

## Goal
Bring the native Goble UI to feature parity with the Tauri-backed prototype by adding:
1. A complete Markdown formatter for chat bubbles.
2. A dedicated Threads view (list + conversation).
3. A User Settings view inspired by Warp's settings_view.

References: `~/Projects/warp-new/crates/markdown_parser`, `~/Projects/warp-new/app/src/settings_view`, `~/Projects/warp-new/app/src/workspace/view/conversation_list`.

---

## 1. Markdown formatter in chat

### 1.1 Research reference implementation
- [x] Read `~/Projects/warp-new/crates/markdown_parser/src/lib.rs` to understand public API.
- [x] Read `~/Projects/warp-new/crates/markdown_parser/src/html_parser.rs` for HTML-to-FormattedText mapping.
- [x] Identify the minimal subset needed for chat: paragraphs, bold, italic, inline code, code blocks, links, unordered/ordered lists, blockquotes.

### 1.2 Extend chat content model
- [x] Add new `ChatFragmentKind` variants in `crates/goble-ui/src/elements/chat_content.rs`:
  - `Bold(String)`
  - `Italic(String)`
  - `BoldItalic(String)`
  - `Link { label: String, url: String }`
  - `List { items: Vec<String>, ordered: bool }`
  - `BlockQuote(String)`
  - `LineBreak`
- [x] Add constructors on `ChatFragment`.
- [x] Keep backward compatibility with existing `Text`, `Code`, `Action`.

### 1.3 Build a markdown parser
- [x] Create `crates/goble-ui/src/elements/markdown.rs`.
- [x] Implement `parse_markdown(input: &str) -> Vec<ChatFragment>` using `pulldown-cmark` or a small hand-written parser.
- [x] Handle nested styles (bold inside italic, links inside lists, etc.).
- [x] Escape raw HTML for safety.

### 1.4 Render formatted fragments
- [x] Update `ChatMessageBubble::rebuild` to render the new fragment kinds:
  - `Bold`/`Italic`/`BoldItalic` via styled `Text`.
  - `Link` as clickable `Text` with `ChatAction::OpenUrl`.
  - `List` as a vertical Flex of labelled rows.
  - `BlockQuote` as indented container with a left border/accent background.
  - `LineBreak` as a zero-height spacer.
- [x] Add `Text::with_weight` / `Text::with_italic` helpers if missing.

### 1.5 Wire formatter into ChatView
- [x] In `ChatView::rebuild`, run plain text through `parse_markdown` before building bubbles.
- [x] Add unit tests for the parser.
- [x] Add layout tests for formatted bubbles.

### 1.6 Verify
- [x] `cargo test -p goble-ui` passes.
- [x] `cargo check -p goble-ui --examples` passes.

---

## 2. Threads view

### 2.1 Domain model wiring
- [x] Confirm `ThreadSummary`, `ThreadMessageSummary`, `ThreadReactionSummary` from `goble-desktop-service` are used.
- [x] Add helper in `goble-desktop-service` (or example) to create a sample thread and messages.

### 2.2 Build `ThreadListView`
- [x] Create `crates/goble-ui/src/views/thread_list_view.rs`.
- [x] Accept `Vec<ThreadSummary>` and a selected callback.
- [x] Reuse `ThreadListItem` for each row.
- [x] Add search/filter header.

### 2.3 Build `ThreadView`
- [x] Create `crates/goble-ui/src/views/thread_view.rs`.
- [x] Display thread title and participant list header.
- [x] Render messages via `ChatMessageBubble` using markdown formatter.
- [x] Add composer for new messages (reuse `ChatComposer`).
- [x] Emit on-send callback with content.

### 2.4 Build `ThreadsContainer`
- [x] Create `crates/goble-ui/src/views/threads_container.rs`.
- [x] Combine `ThreadListView` (left sidebar) and `ThreadView` (right pane) in a horizontal split.
- [x] Track selected thread id.

### 2.5 Update example
- [x] Extend `crates/goble-ui/examples/service_chat.rs` to create threads instead of legacy chat.
- [x] Render `ThreadsContainer` and verify layout.

### 2.6 Verify
- [x] `cargo test -p goble-ui` passes.
- [x] `cargo run -p goble-ui --example service_chat` shows threads.

---

## 3. User Settings view

### 3.1 Research reference implementation
- [x] Read `~/Projects/warp-new/app/src/settings_view/mod.rs` for page structure.
- [x] Read `~/Projects/warp-new/app/src/settings_view/nav.rs` for navigation model.
- [x] Read a few concrete pages (ai_page.rs, appearance_page.rs, features_page.rs) for layout patterns.
- [x] Read `~/Projects/warp-new/app/src/settings_view/settings_page.rs` for shell layout.

### 3.2 Design Goble settings schema
- [x] Create `crates/goble-ui/src/views/settings/settings_model.rs`.
- [x] Define sections: Account, AI Providers, Appearance, Notifications, Security/Cluster, Shortcuts, About.
- [x] Mirror `LlmSetting`, `VaultSecretInfo`, cluster identity flags where relevant.

### 3.3 Build settings primitives
- [x] Add `SettingsRow` element: label + control (text input, select, switch) in `crates/goble-ui/src/elements/settings_row.rs`.
- [x] Add `SettingsSection` element: titled group of rows.
- [x] Add `SettingsNavItem` element.

### 3.4 Build `SettingsView`
- [x] Create `crates/goble-ui/src/views/settings/mod.rs`.
- [x] Implement left nav with sections.
- [x] Implement right pane that shows selected section's rows.
- [x] Add search/filter at the top.

### 3.5 Wire to service layer
- [x] Add example `crates/goble-ui/examples/settings.rs` that loads `DesktopState` and binds LLM settings to the view.
- [x] Add callbacks for saving settings back to service/state.

### 3.6 Verify
- [x] `cargo test -p goble-ui` passes.
- [x] `cargo check -p goble-ui --examples` passes.

---

## 4. Final integration and validation

### 4.1 Service layer integration
- [x] Ensure `DesktopState` exposes all settings needed by the UI.
- [x] Add any missing getters/setters to `goble-desktop-service/src/state.rs`.

### 4.2 Example binary
- [x] Create `crates/goble-ui/src/views/settings/settings_view.rs` as a single-file module.
- [x] Create `crates/goble-ui/examples/full_native_app.rs` that shows a tabbed shell: Threads, Settings, Chat.
- [x] Run the example and verify no panics.

### 4.3 CI green
- [x] `cargo check -p goble-ui`.
- [x] `cargo test -p goble-ui`.
- [x] `cargo check -p goble-desktop-service`.
- [x] `cargo test -p goble-desktop-service`.
- [x] `cargo check` in `crates/goble-desktop/src-tauri`.
- [x] `npm run build` in `crates/goble-desktop`.
