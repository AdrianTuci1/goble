# 019 — Trace/execuții în interiorul unei rutine/agent

## Status

[ ] Activ

## Context

Nu există pagini standalone Executions / AgentTrace. Execuțiile și trace-urile se vizualizează în interiorul panelului de agent/rutină (tab-ul **Trace** din sidebar-ul dreapta). Utilizatorul poate vedea lista de execuții recente și poate intra în detaliile uneia.

## Obiective

1. Creează componenta reutilizabilă `RoutineTracePanel` pentru tab-ul Trace:
   - Listă de execuții (`list_executions`) filtrată după agent/rutină selectată.
   - Sortare implicită după dată, opțional după status.
   - Click pe o execuție deschide vizualizarea detaliată a trace-ului.
2. Implementează vizualizarea trace-ului:
   - Pași executați, tool calls, evenimente.
   - Metrici (timp, tokens, status).
   - Suport pentru streaming parțial / event updates.
3. Reutilizează componente de listă existente și `terminal_block` pentru log-uri.

## Criterii de acceptare

- Se pot lista execuțiile în interiorul tab-ului Trace.
- Click pe execuție arată trace-ul detaliat.
- Build verde.

## Dependințe

- `009-topbar.md`
- `014-agent-right-sidebar-tabs.md`
- `017-routines-in-sidebar.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_sidebar.rs` (tab Trace)
- `crates/goble-ui/src/elements/routine_trace_panel.rs` (nou)
- `crates/goble-desktop-native/src/views/chat.rs`
