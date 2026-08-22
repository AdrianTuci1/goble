# 015 — Pagina de agenți (parity cu Tauri)

## Status

[ ] Activ

## Context

Vederea `AgentManagementView` există deja cu formular de creare/editare brut. Aplicația Tauri (`AgentsPage.tsx`) are un layout cu carduri de agenți, tool tags, butoane Run/Schedule/Edit/Delete și integrare cu sidebar-ul din dreapta.

## Obiective

1. Refactorizează `AgentManagementView` să folosească `AgentCard` premium pentru fiecare agent.
2. Adaugă acțiuni: Run, Schedule, Edit (inline), Delete.
3. Adaugă selecție de agent pentru a afișa detalii în sidebar-ul din dreapta (vezi `014`).
4. Permite alegerea tools/MCP în formularul de editare (listă de tool-uri disponibile).
5. Adaugă un buton "New agent" în topbar sau header.

## Criterii de acceptare

- Lista de agenți arată ca în Tauri.
- Se pot crea, edita, șterge și rula agenți.
- Build verde.

## Dependențe

- `009-topbar.md`
- `011-chat-view.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/agent.rs`
- `crates/goble-ui/src/elements/agent_card.rs`
- `crates/goble-desktop-native/src/app.rs` (routing)
