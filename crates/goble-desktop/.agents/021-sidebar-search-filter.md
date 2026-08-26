# 021 — Search în sidebar, deasupra agenților

## Status

[ ] Activ

## Context

Nu există pagină Search standalone. Căutarea este un câmpl în vârful sidebar-ului stânga, deasupra secțiunii de agenți. Filtrarea se aplică atât agenților cât și rutinelor din sidebar.

## Obiective

1. Adaugă un câmp de **search** în vârful `ConversationSidebar`:
   - Placeholder sugestiv ("Find agents and routines…").
   - Iconiță de search în stânga și clear în dreapta.
   - Debounce la tastare.
2. Filtrează lista de agenți și rutine după:
   - nume,
   - descriere/prompt (pentru agenți),
   - trigger sau pași (pentru rutine).
3. Păstrează ordinea secțiunilor: Search → Agenți → Rutine.
4. Dacă nu există rezultate, arată un empty state.

## Criterii de acceptare

- Search-ul filtrează simultan agenți și rutine.
- Empty state când nu există rezultate.
- Build verde.

## Dependințe

- `009-topbar.md`
- `010-conversation-sidebar.md`
- `012-conversation-sidebar-wiring.md`
- `015-agents-in-sidebar.md`
- `017-routines-in-sidebar.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/conversation_list_item.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
