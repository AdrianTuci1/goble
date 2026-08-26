# 09 — Testing & Validation Checklist

## Unit tests in `goble-ui`

- [ ] `IconAtlas` resolves all required icon names and returns UV rects.
- [ ] `Icon` layout is square and emits a `DrawIcon` command, not `DrawText`.
- [ ] `ConversationListItem` layout includes title, snippet, timestamp.
- [ ] `ConversationListItem` hover state reveals the three-dots menu.
- [ ] Three-dots menu click triggers the delete callback.
- [ ] `TopBar` threads/inbox/user buttons dispatch the correct callbacks.
- [ ] `ChatView` composer send button emits `on_send` with current text.
- [ ] Right chat-sidebar toggle changes visibility state.

## Build validation

- [ ] `cargo test -p goble-ui` passes.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo check -p goble-desktop-native` passes.

## Visual validation (preview example)

- [ ] `cargo run --example preview -p goble-ui` renders the layered shell.
- [ ] Topbar shows traffic lights and right-side controls.
- [ ] Left sidebar shows search, create button, conversation cards, Plugins footer.
- [ ] Conversation cards show hover background and three-dots delete menu.
- [ ] Chat view shows header, messages, and composer.
- [ ] Right chat-sidebar toggles on/off.
- [ ] Threads toggle switches center content to threads list.

## Cleanup

- [ ] Run `cargo clean` after validation to avoid accumulating binaries.
