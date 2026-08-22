# 009 — Topbar nativ premium

## Context
Aplicația nativă `goble-desktop-native` are nevoie de un topbar custom care să oglindească layout-ul din aplicația Tauri (`goble-desktop/src/components/Topbar.tsx`) și stilul premium Warp.

## Obiective
1. Creează o componentă `Topbar` în `goble-ui` cu:
   - Buton meniu (toggle sidebar stânga).
   - Buton Threads (hash icon) cu stare activă.
   - Buton Inbox/Agents (inbox icon) cu stare activă.
   - Buton Settings (gear icon) cu stare activă.
   - Traffic lights pentru macOS (consistență cu `TitleBar` existent).
   - Stări hover/active cu fundal rotunjit și culoare accent.
2. Înlocuiește `TitleBar` în `ShellView` cu noul `Topbar`.
3. Butonul meniu trebuie să:
   - comute `sidebar_collapsed` când suntem în Chat;
   - sau să revină la `ActiveView::Chat` când suntem în Threads/Settings.
4. Păstrează build-ul verde: `cargo test -p goble-ui` și `cargo check -p goble-desktop-native`.

## Criterii de acceptare
- `crates/goble-ui/src/elements/topbar.rs` creat și exportat.
- `ShellView` folosește `Topbar` în loc de `TitleBar`.
- Sidebar stânga poate fi ascuns/afișat din topbar.
- Testele existente trec; se adaugă minim un test pentru layout-ul topbar-ului.

## Fișiere afectate
- `crates/goble-ui/src/elements/topbar.rs` (nou)
- `crates/goble-ui/src/elements.rs`
- `crates/goble-ui/src/lib.rs`
- `crates/goble-ui/src/elements/shell.rs`
- `crates/goble-ui/examples/preview.rs`
