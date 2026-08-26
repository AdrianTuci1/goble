# 014 — Chips în topbar-ul chat-ului pentru rutine deschise

## Status

[ ] Activ

## Context

Când utilizatorul deschide o rutină din sidebar, chat-ul principal devine interfața acelei rutine. Pentru a putea naviga înapoi la agentul principal sau între rutine deschise, topbar-ul chat-ului afișează chips: unul pentru agentul principal și câte unul pentru fiecare rutină deschisă.

## Obiective

1. Adaugă o bandă de **chips** sub headerul chat-ului sau în topbar:
   - Chip-ul principal: **Agent** (mereu primul, nu se poate închide).
   - Chip-uri pentru rutinele deschise: numele rutinei + icon de închidere.
2. Comportamente:
   - Click pe chip → comută view-ul de chat la entitatea respectivă.
   - Click pe `×` al unui chip de rutină → închide rutina și revine la chipul anterior.
   - Dacă se închide ultima rutină, revine la agentul principal.
3. Stochează în `UiState` stiva de rutine deschise (`open_routine_ids: Vec<RoutineId>`).
4. Asigură-te că starea de selecție rămâne consistentă între sidebar și chips.

## Criterii de acceptare

- Rutinele deschise apar ca chips în topbar.
- Se poate comuta între agent și rutine.
- Se pot închide rutine și se revine corect.
- Build verde.

## Dependințe

- `011-chat-view.md`
- `012-routines-sidebar-wiring.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_header.rs` sau `chat_topbar.rs` (nou)
- `crates/goble-ui/src/elements/chip.rs`
- `crates/goble-ui/src/elements/chat_view.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
