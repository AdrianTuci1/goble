# Goble Identity & Provisioning Plan

**Goal:** Implement cluster identity export/import with passphrase encryption and expose UI in Settings > Keys; also expose worker invitation generation in Settings > Compute.

**Branch:** `feature/agent-guide-ui`

**Plan:**

1. `identity-seal` — Implement `export_identity_wallet` and `import_identity_wallet` on `AppState` using the existing `IdentityWallet` encryption primitives.
2. `identity-ui` — Add "Export identity" and "Import identity" buttons in Settings > Keys with passphrase prompts and file download / text paste.
3. `worker-invite-ui` — Add a "Generate worker invite" button in Settings > Compute that produces a base64 invite containing cluster key and worker mTLS bundle.
4. `cleanup-plans` — Mark outdated plans (`worker-full-runtime.md`, `goble-ui-gap-plan.md`, `goble-ui-polish-roadmap.md`) as superseded/deprecated or delete them.
5. `verify` — Run full frontend lint + build + tests, backend cargo check + clippy + tests, then push.

**Files:**
- `crates/goble-desktop/src-tauri/src/state.rs`
- `crates/goble-desktop/src-tauri/src/lib.rs` (commands)
- `crates/goble-desktop/src-tauri/src/commands.rs` (if exists, or lib.rs)
- `crates/goble-desktop/src/tauri/api.ts`
- `crates/goble-desktop/src/pages/SettingsPage.tsx`
- `crates/goble-desktop/src/pages/Pages.css`
- `docs/plans/goble-identity-team-plan.md` (this file)

**Verification command:**
```bash
npx eslint src && npm run build && npm run test && cargo clippy -p goble-desktop-tauri --lib -D warnings && cargo test -p goble-desktop-tauri --lib && cargo test -p goble-core
```


---

## Verification (auto-run after implementation)

- `npx eslint src` → ✅ 0 errors, 0 warnings
- `npm run build` → ✅
- `cargo clippy -p goble-desktop-tauri --lib -D warnings` → ✅
- `cargo test -p goble-desktop-tauri --lib` → ✅ 23 passed
- `cargo test -p goble-core` → ✅ 149 passed

Run manually: `cd /root/goble/crates/goble-desktop && npm run test`
