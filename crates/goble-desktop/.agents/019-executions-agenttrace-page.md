# 019 — Pagini Executions și AgentTrace

## Status

[ ] Activ

## Context

Tauri are `ExecutionsPage.tsx` (listă de execuții) și `AgentTracePage.tsx` (vizualizare detaliată a unei execuții cu log-uri, tool call-uri, metrici). Nativ nu există aceste pagini.

## Obiective

1. Creează `ActiveView::Executions` și `ActiveView::AgentTrace`.
2. Implementează `ExecutionsViewPanel`:
   - Listează execuțiile (`list_executions`).
   - Sortare după dată, status.
   - Click pe execuție deschide trace-ul.
3. Implementează `AgentTraceViewPanel`:
   - Afișează pașii, log-urile, metricile și evenimentele trace-ului (`get_execution_trace`).
   - Suport pentru streaming parțial (event updates).
4. Adaugă buton de navigare în topbar/sidebar.

## Criterii de acceptare

- Se pot lista execuțiile și vizualiza un trace.
- Build verde.

## Dependențe

- `009-topbar.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/executions.rs` (nou)
- `crates/goble-desktop-native/src/views/agent_trace.rs` (nou)
- `crates/goble-desktop-native/src/app.rs`
- `crates/goble-ui/src/elements/shell.rs`
