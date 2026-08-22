# 013 — Îmbunătățiri composer chat

## Status

[ ] Activ

## Context

Composer-ul actual are doar atașare și trimite. Aplicația Tauri include selector de model/provider, selector de runtime/target, și un card special pentru salvarea cheii API când modelul nu este configurat.

## Obiective

1. Adaugă butoane în `ChatComposer` pentru:
   - Model/provider (apelând `set_chat_model` / `get_llm_setting`).
   - Runtime target (`auto`, `local`, `tag`, `worker`).
   - Variant/temperature (opțional, placeholder).
2. Implementează `Enter` pentru trimitere și `Shift+Enter` pentru newline în `TextArea`.
3. Detectează în `ChatViewPanel` când un chat nu are provider/model setat și afișează un card inline pentru introducerea cheii API (similar `ApiKeyCard` din React).
4. Salvează setările LLM via `state_api::set_llm_setting` și actualizează chat-ul cu provider/model default.

## Criterii de acceptare

- Utilizatorul poate selecta modelul din composer.
- Enter trimite mesajul; Shift+Enter adaugă o linie nouă.
- Cardul de API key apare când nu există cheie/model.
- Build-ul rămâne verde.

## Dependențe

- `011-chat-view.md`
- `012-conversation-sidebar-wiring.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_composer.rs`
- `crates/goble-ui/src/elements/text_area.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-ui/src/elements/chat_message_bubble.rs` (opțional, pentru card)
