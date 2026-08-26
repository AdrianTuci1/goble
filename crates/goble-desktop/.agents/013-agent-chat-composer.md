# 013 — Composerul chat-ului principal: limbaj natural + slash extras

## Status

[ ] Activ

## Context

Chat-ul principal este shell-ul agentului. Utilizatorul scrie în limbaj natural, dar există și comenzi cu slash ca extra. Composerul trebuie să susțină input multi-line, să afișeze sugestii pentru slash, și să permită agentului să răspundă și să scaffoldeze rutine.

## Obiective

1. Refactorizează `ChatComposer` pentru input natural în `TextArea`:
   - Placeholder prietenos („Ask me anything…”).
   - `Enter` trimite mesajul; `Shift+Enter` adaugă o linie nouă.
   - Suport pentru anexări (fișiere, imagini) — opțional.
2. Adaugă suport pentru **slash commands** ca extra:
   - `/new` — începe sesiune nouă în chat-ul infinit (opțional).
   - `/routine <name>` — cere agentului să creeze/modifice o rutină.
   - `/verify`, `/modify`, `/delete` — interacțiuni cu rutinele (când este cazul).
   - Meniu de sugestii la tastarea `/`.
3. Detectează când modelul/provider-ul nu este configurat și afișează un card inline pentru introducerea cheii API.
4. Expune callback-uri către `ChatViewPanel`: `on_send`, `on_slash_command`.

## Criterii de acceptare

- Limbaj natural funcționează în rich input.
- Slash commands sunt recunoscute și dispatch-uite.
- Enter/Shift+Enter au comportamentul corect.
- Cardul de API key apare când nu există model/key.
- Build verde.

## Dependințe

- `011-chat-view.md`
- `012-routines-sidebar-wiring.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_composer.rs`
- `crates/goble-ui/src/elements/text_area.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
