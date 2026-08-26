# Teste de integrare pentru aplicația Goble

Testele din acest director conduc fluxul real al aplicației (starea + callback-urile din
`app/src`) contra unui backend `goble-desktop-service` real, cu un store SQLite în memorie
și un director temporar pentru thread store. Pentru a le putea accesa, modulul aplicației
(`app/src/lib.rs`) expune acum modulele `state`, `actions`, `ai` și `root_view`.

## Cum se rulează

```bash
cargo test -p goble-app
```

## Ce acoperă fiecare fișier

| Fișier | Zonă funcțională | Teste |
| --- | --- | --- |
| `chat_flow.rs` | Conversații: creare, trimitere mesaj, comutare între conversații, schimbare tab | creare și persistare, titlu gol ignorat, mesaj adăugat + persistat, refresh la selecție, schimbare tab |
| `cron_flow.rs` | Task-uri programate (cron/workflow): creare, ștergere, declanșare | cron adăugat în UI + backend, ștergere, declanșare, CRUD direct pe `DesktopState` |
| `vault_flow.rs` | Secrete (vault): deblocare, adăugare, ștergere, erori | deblocare + adăugare + ștergere, cheie goală → eroare, adăugare înainte de deblocare → eroare |
| `connector_flow.rs` | Conectori MCP: instalare, descoperire unelte, activare/dezactivare, ștergere | instalare + închidere sertar, nume gol → eroare, descoperire + toggle, ștergere |
| `ui_render.rs` | Render întreg shell-ul UI prin `RootView` | randează fără panică pe backend gol și cu mesaje |
| `first_run_flow.rs` | Primul rulaj: configurare cheie model -> setări LLM -> alegere local/remote | banner la primul mesaj fără cheie, navigare la setări, alegere workspace |

## Note de implementare

- `connector_flow.rs` folosește calea *mock* (fără backend) pentru că instalarea reală a unui
  conector rulează `npm` pe rețea — testul urmărește tranzițiile de stare ale aplicației, nu
  instalarea efectivă.
- Backendul este reconstruit pentru fiecare test via `common::desktop_state()`, deci testele sunt
  izolate între ele. `common.rs` se partajează între fișiere prin `mod common;`.

## Idei de teste viitoare

- Flux de workeri: adăugare + pairing + mesaj (necesită un server WebSocket de test, vezi
  `crates/goble-desktop/src-tauri/tests/integration_test.rs`).
- Ro-unda de persistență (store reopen) pentru chat-uri, workflow-uri și secrete.
- Identitate de cluster: creare/import cheie, generare invitație worker, `helm install`.
- Backend de mesaje agent (execuții): `AgentStarted`/`AgentFinished`/loguri prin `handle_worker_message`.
