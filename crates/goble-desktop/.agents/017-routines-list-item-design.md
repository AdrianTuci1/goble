# 017 — Designul vizual al item-ilor de rutină în sidebar

## Status

[ ] Activ

## Context

Sidebar-ul stânga conține doar rutine. Item-ul de rutină trebuie să arate clar numele, starea (activ/inactiv), trigger-ul (manual/cron/etc.) și să ofere acțiuni rapide (toggle, ștergere). Designul trebuie să fie premium, în linie cu warp-new.

## Obiective

1. Creează/rafiniază componenta `RoutineListItem`:
   - Iconiță reprezentativă pentru rutină.
   - Numele rutinei și o linie secundară cu trigger/status.
   - Indicator vizual pentru rulare în desfășurare (dot/puls).
   - Stare `selected` evidențiată.
2. Adaugă acțiuni rapide la hover sau într-un meniu contextual:
   - Toggle enabled/disabled.
   - Ștergere.
   - Editare (deschide rutina în chat).
3. Stări empty state pentru când nu există rutine.
4. Asigură-te că item-ul funcționează în sidebar resizable (text trunchiat, elipsis).

## Criterii de acceptare

- `RoutineListItem` arată premium și afișează toate informațiile relevante.
- Acțiunile rapide sunt accesibile.
- Sidebar resizable nu strică layout-ul item-ului.
- Build verde.

## Dependințe

- `010-conversation-sidebar.md`
- `012-routines-sidebar-wiring.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/routine_list_item.rs`
- `crates/goble-ui/src/elements/conversation_sidebar.rs`
