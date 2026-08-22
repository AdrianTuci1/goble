# 020 — Pagina Logs

## Status

[ ] Activ

## Context

Tauri are `LogsPage.tsx` pentru vizualizarea log-urilor workerilor (`worker_logs`). Nativ nu există această pagină.

## Obiective

1. Creează `ActiveView::Logs` și navigare în topbar/sidebar.
2. Implementează `LogsViewPanel`:
   - Listează log-urile (`worker_logs`).
   - Refresh la evenimentul `logs:updated`.
   - Filtrare simplă după text.
3. Reutilizează componente de listă existente.

## Criterii de acceptare

- Log-urile sunt vizibile în aplicația nativă.
- Se actualizează automat.
- Build verde.

## Dependențe

- `009-topbar.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/logs.rs` (nou)
- `crates/goble-desktop-native/src/app.rs`
- `crates/goble-ui/src/elements/shell.rs`
