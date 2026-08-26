# 012 — Sidebar stânga cu rutine reale și resizable

## Status

[ ] Activ

## Context

Sidebar-ul stânga (`ConversationSidebar`) afișează acum date de test (`sample_conversations`). În noua direcție conține **doar rutine** (workflow-uri). Trebuie să fie legat la date reale, să fie **resizable** la fel ca sidebar-ul principal, și să permită deschiderea unei rutine în chat-ul principal.

## Obiective

1. Refactorizează `ConversationSidebar` să afișeze doar secțiunea **Rutine**:
   - Header cu numărul de rutine și buton `+` pentru rutină nouă.
   - Listă de `RoutineListItem` cu nume, trigger, stare activ/inactiv.
   - Stare `selected` sincronizată cu `UiState.selected_routine_id`.
2. Propagă `DesktopState` și `UiState` către `ShellView` pentru a putea selecta rutina activă.
3. Implementează callback-uri:
   - `on_select` — setează `selected_routine_id` și deschide rutina în chat-ul principal.
   - `on_create` — creează o rutină goală și o deschide în chat.
   - `on_toggle_enabled` — activează/dezactivează rutina.
   - `on_delete` — șterge rutina după confirmare.
4. Asigură-te că sidebar-ul este **resizable** (draggable splitter, minim/maxim width).
5. Click pe o rutină trimite `RoutineOpened(routine_id)`; `ChatViewPanel` comută în modul de vizualizare a rutinei.

## Criterii de acceptare

- Sidebar-ul afișează rutine reale din `DesktopState`.
- Sidebar-ul este resizable.
- Selectarea unei rutine o deschide în chat.
- Se pot crea, șterge și activa/dezactiva rutine.
- `cargo test -p goble-ui` și `cargo check -p goble-desktop-native` rămân verzi.

## Dependențe

- `010-conversation-sidebar.md` (UI deja construit)
- `011-chat-view.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/shell.rs`
- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/routine_list_item.rs` (nou)
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/state_api.rs` (pentru list_workflows, create_workflow, delete_workflow, toggle)
