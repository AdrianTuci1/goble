# Agent 00 — Vision și principii

## Scop
Goble este un sistem de agenți autonomi care combină o aplicație desktop nativă (Rust + gpui) cu un worker (Goblin) rulat pe VPS. Utilizatorul creează agenți în limbaj natural, îi activează local sau remote, și îi monitorizează.

## Principii arhitecturale
1. **No shortcuts.** Fiecare strat este scris în Rust, testat 100%, fără dependențe de frameworkuri dinamice.
2. **UI nativ, custom.** wgpu + winit + renderer custom, stil minimal, design tokenuri, animații subtile.
3. **Security first.** Pairing code, mTLS, criptare credentiale, sandbox V8 Isolate.
4. **Multi-provider LLM.** Abstractizare `LlmProvider`, implementări pentru OpenAI, Anthropic, Ollama, OpenRouter.
5. **Worker ca binar.** Goblin este compilat static (musl), livrat prin SSH, auto-actualizat.
6. **Sandbox cu V8 Isolate.** Agenții și MCP-urile rulează în V8 Isolate, nu în Docker: start instant, consum redus, fără daemon.
7. **Agentul este cod.** Configurarea în limbaj natural se transformă în spec `Agent`, nu în execuție directă.
8. **Observabilitate.** Fiecare execuție devine un `ExecutionTrace` vizibil în UI.

## Personaje
- **Goble** — mascotă desktop, ghid, prietenosă.
- **Goblin** — mascota worker, silențios, eficient.

## Lifecycle agent
1. User descrie agentul în chat.
2. LLM generează `AgentSpec`.
3. User confirmă / editează în wizard.
4. Goble salvează local și trimite la Goblin(workerii selectați).
5. Goblin rulează la trigger (cron, HTTP, manual).
6. Rezultatele și trace-ul revin în Goble.

## Decizii deja luate
- Licență MIT.
- Workspace Cargo, crate-uri separate.
- UI cu wgpu + winit + renderer custom.
- Coverage 100% înainte de a porni clientul UI.
- Folder `.agents/` pentru planificare.

## Următoarele fișiere
- `01-goble-core.md` — protocol, crypto, LLM abstractions, modelul agentului.
- `02-goblin-worker.md` — server, runner, scheduler, registry, observability.
- `03-goble-desktop.md` — wgpu window, state, chat, views, wizard, worker manager.
- `04-goble-ui.md` — design system, componente reutilizabile.
- `05-goble-cli.md` — CLI pentru worker.
- `06-testing.md` — strategie de teste, coverage, e2e.
- `07-roadmap.md` — ordinea de implementare.
