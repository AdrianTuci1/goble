# 021 — Pagina Search

## Status

[ ] Activ

## Context

Tauri are `SearchPage.tsx` pentru căutare în conversații și execuții. Nativ nu există.

## Obiective

1. Creează `ActiveView::Search` și navigare (probabil din sidebar).
2. Implementează `SearchViewPanel`:
   - Input de căutare.
   - Căutare în conversații (după titlu/conținut mesaje) și execuții.
   - Rezultate grupate cu click pentru navigare la chat/execuție.
3. Dacă backend-ul nu are API dedicat, implementează căutarea locală în memorie.

## Criterii de acceptare

- Pagina Search este navigabilă.
- Returnează rezultate relevante.
- Build verde.

## Dependențe

- `009-topbar.md`
- `012-conversation-sidebar-wiring.md`
- `019-executions-agenttrace-page.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/search.rs` (nou)
- `crates/goble-desktop-native/src/app.rs`
- `crates/goble-ui/src/elements/shell.rs`
