# 018 — Pagina Teams

## Status

[ ] Activ

## Context

`TeamsPage.tsx` din Tauri permite crearea de echipe, adăugarea de agenți și editarea metadatelor. Nativ nu există UI dedicat.

## Obiective

1. Creează `ActiveView::Teams` și intrare în topbar/sidebar.
2. Implementează `TeamsViewPanel`.
3. Funcționalități:
   - Listare echipe (`list_teams`).
   - Creare echipă cu nume, metadata și selecție agenți (`create_team`).
   - Vizualizare membri.
4. Reutilizează componente existente (carduri, avatar, listă).

## Criterii de acceptare

- Pagina Teams este navigabilă.
- Se pot crea și lista echipe.
- Build verde.

## Dependințe

- `009-topbar.md`
- `015-agents-page.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/teams.rs` (nou)
- `crates/goble-desktop-native/src/app.rs`
- `crates/goble-ui/src/elements/shell.rs`
