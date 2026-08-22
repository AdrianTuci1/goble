# 017 — Pagina Workflows

## Status

[ ] Activ

## Context

`WorkflowsPage.tsx` din Tauri permite crearea, listarea și gestionarea workflow-urilor (pași, dependențe, trigger cron, enable/disable). Nativ, workflow-urile sunt doar în Drive, fără UI dedicat.

## Obiective

1. Creează `ActiveView::Workflows` și o intrare în topbar/sidebar.
2. Implementează `WorkflowsViewPanel`.
3. Funcționalități:
   - Listare workflow-uri.
   - Creare workflow cu pași, dependențe și trigger.
   - Editare pași (selecție agent per pas).
   - Delete și toggle enabled.
4. Reutilizează `ConnectorCard` sau un card nou pentru workflow.

## Criterii de acceptare

- Workflow-urile sunt listate și editabile.
- Se pot crea și șterge workflow-uri.
- Build verde.

## Dependențe

- `009-topbar.md`
- `015-agents-page.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/workflows.rs` (nou)
- `crates/goble-desktop-native/src/app.rs`
- `crates/goble-ui/src/elements/shell.rs`
