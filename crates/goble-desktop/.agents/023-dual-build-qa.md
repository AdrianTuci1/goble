# 023 — Dual build și QA final

## Status

[ ] Activ

## Context

Obiectivul final este ca ambele aplicații (Tauri și nativă) să ruleze în paralel și să treacă de verificări.

## Obiective

1. Verifică că `cargo run --bin goble-native` pornește fără erori.
2. Verifică că `npx tauri dev` încă funcționează (fără regresii în build-ul Tauri).
3. Rulează `cargo test -p goble-ui`, `cargo check -p goble-desktop-native`, `cargo check -p goble-ui --examples`.
4. Verifică workflow-urile CI existente pentru Tauri și Rust.
5. Rezolvă warning-uri și compile errors introduse de task-urile anterioare.

## Criterii de acceptare

- Ambele aplicații se compilează și pornesc.
- Toate testele trec.
- Nu există warning-uri critice.

## Dependențe

- Toate task-urile anterioare.

## Fișiere afectate

- Toate fișierele modificate anterior.
- Eventuale CI config files.
