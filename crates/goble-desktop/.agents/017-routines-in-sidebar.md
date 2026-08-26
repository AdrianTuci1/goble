# 017 — Workflow-urile apar ca rutine în sidebar

## Status

[ ] Activ

## Context

Workflow-urile nu mai au pagină dedicată. În noua direcție apar ca **rutine** în sidebar-ul de conversație, sub secțiunea de agenți. O rutină este un workflow salvat care poate fi rulat manual sau prin trigger. Când e selectată, view-ul principal arată panoul de execuție al rutinei.

## Obiective

1. Creează secțiunea **Rutine** în `ConversationSidebar` sub secțiunea **Agenți**:
   - Header cu numărul de rutine și buton `+` pentru rutină nouă.
   - Listă de `RoutineListItem` cu nume, trigger, stare activ/inactiv.
   - Stare `selected` sincronizată cu `UiState.selected_routine_id`.
2. Implementează callback-uri:
   - `on_select` — deschide panoul de execuție/trace al rutinei.
   - `on_create` — pornește editorul de rutină (poate fi o variantă a composerului de agent).
   - `on_toggle_enabled` — activează/dezactivează rutina.
   - `on_delete` — șterge rutina după confirmare.
3. Expune în `state_api` metodele necesare: `list_routines`, `create_routine`, `delete_routine`, `toggle_routine`.
4. Reutilizează `ConnectorCard` sau creează `RoutineListItem` pentru UI.

## Criterii de acceptare

- Rutinele sunt listate în sidebar sub agenți.
- Selectarea unei rutine deschide panoul de execuție/trace.
- Se pot crea, șterge și activa/dezactiva rutine.
- Build verde.

## Dependințe

- `009-topbar.md`
- `011-chat-view.md`
- `012-conversation-sidebar-wiring.md`
- `015-agents-in-sidebar.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/routine_list_item.rs` (nou)
- `crates/goble-ui/src/elements/connector_card.rs` (reutilizat)
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/state_api.rs`
