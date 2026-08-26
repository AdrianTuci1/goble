# 015 — Trace/log-uri ale rutinei afișate în view-ul de chat

## Status

[ ] Activ

## Context

Nu există pagini standalone Executions / Logs / AgentTrace. Când o rutină este deschisă, chat-ul principal îi afișează trace-ul și log-urile (istoricul execuțiilor). View-ul rămâne un chat cu rich input, dar istoricul mesajelor este înlocuit cu pașii execuției, tool calls și log-uri.

## Obiective

1. Adaugă un mod de vizualizare **Trace** în `ChatViewPanel` pentru rutine:
   - Listă de execuții recente ale rutinei (`list_executions` filtrat după `routine_id`).
   - Sortare implicită descrescătoare după dată.
   - Click pe o execuție deschide vizualizarea detaliată.
2. Implementează vizualizarea detaliată a execuției:
   - Pași executați, tool calls, evenimente.
   - Log-uri worker (filtrate pentru execuție).
   - Metrici (timp, tokens, status).
   - Suport streaming parțial / updates.
3. Reutilizează componentele existente: `terminal_block`, `chat_message_bubble`, liste.
4. Rich input-ul rămâne activ pentru a putea interacționa cu rutina în limbaj natural.

## Criterii de acceptare

- Când o rutină este selectată, chat-ul îi arată trace-ul/log-urile.
- Se pot lista execuțiile și vizualiza detaliile.
- Build verde.

## Dependințe

- `011-chat-view.md`
- `012-routines-sidebar-wiring.md`
- `014-routine-chips.md`

## Fișiere afectate

- `crates/goble-ui/src/views/chat_view.rs`
- `crates/goble-ui/src/elements/chat_message_bubble.rs`
- `crates/goble-ui/src/elements/terminal_block.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
