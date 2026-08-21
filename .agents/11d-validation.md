# 11d — Validation checklist

Part of: `11-warp-native-redesign-master.md`

## Goal
Ensure the native app and `goble-ui` changes compile and existing tests still pass.

## Commands

```bash
cargo check --workspace --all-targets
cargo test -p goble-ui
cargo test -p goble-desktop-service
cd crates/goble-desktop && npm run build
```

## Checklist
- [ ] `cargo check -p goble-desktop-native` passes.
- [ ] `cargo check --workspace --all-targets` passes.
- [ ] `cargo test -p goble-ui` passes.
- [ ] `cargo test -p goble-desktop-service` passes.
- [ ] `npm run build` in `crates/goble-desktop` still passes (React app untouched).
- [ ] Native app window opens and renders the shell.
- [ ] Topbar drag and buttons work on macOS.
- [ ] Sidebar switches between Agent conversations / Threads / Drive.
- [ ] Agent management panel lists and filters runs.
- [ ] Drive shows Plans, Rules, Workflows.
- [ ] Settings tabs render including Cluster.
