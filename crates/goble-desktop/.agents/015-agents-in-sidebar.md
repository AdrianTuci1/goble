# 015 — Agenții apar în sidebar, nu ca pagină separată

## Status

[ ] Activ

## Context

În noua direcție aplicația nu are o pagină “Agenți” standalone. Agenții sunt entități de primă clasă în sidebar-ul stânga, alături de rutine. Utilizatorul poate vedea agenții creați, poate selecta unul pentru editare sau poate crea unul nou.

## Obiective

1. Creează/actualizează secțiunea **Agenți** în `ConversationSidebar`:
   - Header cu numărul de agenți și buton `+` pentru agent nou.
   - Listă de `AgentListItem` cu nume, scurtă descriere, icon/avatar.
   - Stare `selected` sincronizată cu `UiState.selected_agent_id`.
2. Implementează callback-uri:
   - `on_select` — setează `selected_agent_id`, deschide editorul de agent în view-ul principal.
   - `on_create` — creează un agent gol, îl selectează și îl deschide în editor.
   - `on_delete` — șterge agentul după confirmare.
3. Reutilizează `AgentCard` / `Avatar` pentru iconița agentului.
4. Asigură-te că datele vin din `DesktopState::list_agents()` (sau echivalentul din `state_api`).

## Criterii de acceptare

- Agenții sunt listați în sidebar sub secțiunea de search.
- Selectarea unui agent deschide editorul în view-ul principal.
- `+` creează un agent nou.
- Ștergerea unui agent îl elimină din listă.
- Build verde.

## Dependențe

- `009-topbar.md`
- `011-chat-view.md`
- `012-conversation-sidebar-wiring.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/agent_list_item.rs` (nou)
- `crates/goble-ui/src/elements/agent_card.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/state_api.rs`
