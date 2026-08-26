# 012 — Legarea sidebar-ului cu date reale (agenți + rutine + search)

## Status

[ ] Activ

## Context

Sidebar-ul din stânga (`ConversationSidebar`) afișează acum date de test (`sample_conversations`). În noua direcție, sidebar-ul este navigatorul principal al aplicației: în partea de sus are un câmp de search, apoi lista de **agenți** și lista de **rutine** (workflow-uri). Selecția din sidebar determină ce se afișează în view-ul principal: un agent în mod editor sau o rutină în mod execuție.

## Obiective

1. Refactorizează `ShellView::left_panel` să încarce agenți reali și rutine (workflow-uri) din `DesktopState`.
2. Propagă `DesktopState` și `UiState` către `ShellView` pentru a putea selecta agentul/rutina activă.
3. Actualizează `ConversationSidebar` / `ConversationListItem` (sau creează componente noi `AgentListItem`, `RoutineListItem`) să lucreze cu date reale:
   - Agenți: nume, scurtă descriere, icon/avatar, stare `selected`.
   - Rutine: nume, stare `selected`, badge dacă au execuții recente.
4. Adaugă câmpul de **search** în vârful sidebar-ului — filtrează agenți și rutine după nume/descriere.
5. Butonul `+` din header creează un **agent nou** și deschide editorul în view-ul principal.
6. Click pe un agent deschide editorul de agent; click pe o rutină deschide panoul de execuție/trace al rutinei.

## Criterii de acceptare

- Sidebar-ul afișează agenți și rutine reale din baza de date locală.
- Search-ul filtrează agenți + rutine.
- Click pe agent/rutină schimbă selecția și actualizează view-ul principal.
- `+` creează un agent nou gol în view-ul principal.
- `cargo test -p goble-ui` și `cargo check -p goble-desktop-native` rămân verzi.

## Dependențe

- `010-conversation-sidebar.md` (UI deja construit)
- `011-chat-view.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/shell.rs`
- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/conversation_list_item.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/state_api.rs` (pentru listă agenți, listă rutine, search)
