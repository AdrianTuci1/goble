# 020 — Log-uri în interiorul vizualizării de rutină/agent

## Status

[ ] Activ

## Context

Nu există pagină Logs standalone. Log-urile workerilor / execuțiilor se văd în contextul unei rutine sau al unui agent, de obicei în tab-ul Trace sau într-o secțiune expandabilă a panelului de execuție.

## Obiective

1. Adaugă o secțiune de **log-uri** în `RoutineTracePanel` (sau în tab-ul Trace):
   - Afișează log-urile asociate execuției/agentului selectat (`worker_logs` filtrate).
   - Refresh la evenimentul `logs:updated`.
   - Filtrare simplă după text.
   - Folosește componentele existente de listă / terminal block pentru redare.
2. Asigură-te că log-urile pot fi vizualizate și în timpul unei execuții în desfășurare.

## Criterii de acceptare

- Log-urile sunt vizibile în interiorul panelului de rutină/agent.
- Se actualizează automat la evenimente noi.
- Build verde.

## Dependințe

- `009-topbar.md`
- `014-agent-right-sidebar-tabs.md`
- `019-routine-trace-panel.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/routine_trace_panel.rs`
- `crates/goble-ui/src/elements/chat_sidebar.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
