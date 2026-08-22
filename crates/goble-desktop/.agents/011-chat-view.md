# 011 — Chat view

## Status

[x] Finalizat

## Context

După implementarea topbar-ului (`009`) și a sidebar-ului de conversație (`010`), următorul pas a fost construirea view-ului principal de chat: un header de chat, un content pentru conversație nouă și un composer.

## Obiective

- Creează un `ChatHeader` reutilizabil în `goble-ui` cu titlul conversației și buton de toggle pentru sidebar-ul din dreapta.
- Actualizează `ChatView` să afișeze un empty state când nu există mesaje.
- Îmbunătățește stilul `ChatComposer` folosind `TopbarButton` pentru atașare/trimitere și un container cu fundal/bordură.
- În `goble-desktop-native`, porneste mereu într-o conversație nouă și deschide sidebar-ul de chat din header.
- Actualizează exemplul `preview.rs` pentru a reflecta starea nouă.

## Criterii de acceptare

- `cargo test -p goble-ui` trece.
- `cargo check -p goble-desktop-native` trece.
- `cargo check -p goble-ui --examples` trece.

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_header.rs` (nou)
- `crates/goble-ui/src/elements/chat_composer.rs`
- `crates/goble-ui/src/views/chat_view.rs`
- `crates/goble-ui/src/elements.rs`
- `crates/goble-ui/src/lib.rs`
- `crates/goble-ui/examples/preview.rs`
- `crates/goble-desktop-native/src/views/chat.rs`

## Note

- Nu s-au modificat parserul de text sau renderele custom pentru mesaje.
