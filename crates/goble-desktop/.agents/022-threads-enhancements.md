# 022 — Îmbunătățiri Threads

## Status

[ ] Activ

## Context

Vederea nativă de Threads este funcțională (listă + mesaje). Tauri are funcții suplimentare: participanți, reacții, mentions, reply-to, thread types (channel/direct/chat), mark as read.

## Obiective

1. Adaugă management de participanți în `ThreadsViewPanel` (add/remove, invite by public key).
2. Adaugă reacții la mesaje (`add_thread_reaction` / `remove_thread_reaction`).
3. Adaugă reply-to și mentions.
4. Adaugă mark thread as read și unread count.
5. Îmbunătățește UI-ul `ThreadsContainer` pentru a afișa canal vs chat vs direct.

## Criterii de acceptare

- Threads acceptă participanți, reacții și reply-uri.
- Unread count este calculat.
- Build verde.

## Dependențe

- `011-chat-view.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/threads.rs`
- `crates/goble-ui/src/views/threads_container.rs`
- `crates/goble-ui/src/elements/thread_list_item.rs`
