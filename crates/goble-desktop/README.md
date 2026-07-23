# Goble Desktop

Aplicația desktop Goble, construită cu Tauri + React + Vite.

## Structură

- `src/` — frontend React (componente adaptate din Rynd)
- `src-tauri/` — backend Rust (Tauri commands, state, worker client)
- `public/` — asset-uri statice

## Comenzi

```bash
cd crates/goble-desktop
npm install
npm run dev          # development Vite (fără Tauri shell)
npx tauri dev        # desktop Tauri dev
npm run build        # production frontend
npx tauri build      # build complet aplicație desktop
```

## Backend commands

- `list_workers` — lista workerilor conectați
- `worker_logs` — loguri worker
- `ping_worker` — ping la un worker
- `add_log` — adaugă log în state

## Note

- `crates/goble-desktop` este exclus din workspace-ul Cargo pentru că are propriul `Cargo.toml` Tauri în `src-tauri/`.
- Proiectul folosește stil monocrom flat, fără umbre, conform preferințelor de design.
