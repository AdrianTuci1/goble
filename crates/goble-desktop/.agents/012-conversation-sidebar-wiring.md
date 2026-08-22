# 012 — Wiring sidebar conversație cu date reale

## Status

[ ] Activ

## Context

Sidebar-ul de conversație (`ConversationSidebar`) afișează acum doar date de test (`sample_conversations`). Aplicația Tauri listează conversațiile reale, permite crearea uneia noi, selectarea și ștergerea. Trebuie să legăm `ShellView` și `ChatViewPanel` de `DesktopState`.

## Obiective

1. În `ShellView::left_panel`, încarcă lista reală de chats via `state_api::list_chats` (sau `state.list_chats()`).
2. Propagă `DesktopState` și `UiState` către `ShellView` pentru a putea selecta conversația activă.
3. Actualizează `ConversationSidebar` și `ConversationListItem` să lucreze cu date reale:
   - titlu, ultimul mesaj și timestamp extrase din `Chat` / `ChatMessage`.
   - stare `selected` bazată pe `UiState.selected_chat_id`.
   - callback `on_select` setează `selected_chat_id` și marchează `dirty`.
   - callback `on_delete` șterge conversația (adaugă `delete_chat` în service sau state_api dacă lipsește).
4. Butonul `+` din header creează un chat nou (`create_chat`) și îl selectează.
5. `ChatViewPanel` trebuie să pornească în conversația selectată; dacă nu există niciuna, creează una nouă (deja implementat parțial).

## Criterii de acceptare

- Sidebar-ul afișează conversațiile reale din baza de date locală.
- Click pe o conversație o deschide în ChatView.
- `+` creează o conversație nouă.
- `cargo test -p goble-ui` și `cargo check -p goble-desktop-native` rămân verzi.

## Dependențe

- `010-conversation-sidebar.md` (UI deja construit)
- `011-chat-view.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/shell.rs`
- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/conversation_list_item.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/state_api.rs` (opțional, pentru `delete_chat`)
