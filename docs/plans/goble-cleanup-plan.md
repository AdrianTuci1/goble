# Goble Cleanup Plan

**Goal:** Close remaining code-quality gaps: fix clippy warnings, clean up SettingsPage placeholders, replace placeholder text, and refactor ChatArea trace link.

**Branch:** `feature/agent-guide-ui`

**Order of execution:**
1. `clippy-fix` — Fix all `cargo clippy -p goble-desktop-tauri --lib -D warnings` errors.
2. `settings-cleanup` — Remove or replace SettingsPage placeholder tabs; replace QR placeholder text.
3. `chatarea-navigate` — Refactor ChatArea trace link to use `useNavigate`.
4. `cleanup-verify` — Run full verification and push.

---

## Task 1: Fix cargo clippy warnings

**Objective:** `cargo clippy -p goble-desktop-tauri --lib -D warnings` must exit 0.

**Known issues:**
- `manual_strip` in `extractMentions`.
- `redundant_closure` in `thread_store.rs:491` and `lib.rs:995`.
- `useless_format` in `thread_store.rs:610`.
- Plus other warnings surfaced by clippy.

**Files:**
- `crates/goble-desktop/src-tauri/src/thread_store.rs`
- `crates/goble-desktop/src-tauri/src/lib.rs`
- Other clippy-flagged files.

**Verification:** `cargo clippy -p goble-desktop-tauri --lib -D warnings`

---

## Task 2: SettingsPage placeholder cleanup

**Objective:** Replace or remove placeholder Settings tabs so the menu feels intentional.

**Actions:**
- Remove menu entries for tabs that are genuinely not useful yet: `members`, `hosted-communities`, `templates`, `invites`, `experiments`.
- Keep `shortcuts` but render a useful "Keyboard shortcuts" page listing available shortcuts (even if only basic ones like `Cmd/Ctrl+K` search, `Esc` close panels).
- Replace Mobile QR placeholder text with "Mobile pairing will be available once the companion app is released." and remove the placeholder `<div>`.

**Files:**
- `crates/goble-desktop/src/pages/SettingsPage.tsx`
- `crates/goble-desktop/src/pages/Pages.css`

**Verification:** `npm run build` passes; UI still works.

---

## Task 3: ChatArea useNavigate refactor

**Objective:** Make ChatArea trace link use React Router navigation instead of `useStore.getState().navigateFn`.

**Actions:**
- Add `useNavigate` import.
- Replace `useStore.getState().navigate('/traces')` with `navigate('/traces')`.

**Files:**
- `crates/goble-desktop/src/components/ChatArea.tsx`

**Verification:** `npm run build` passes, eslint passes.

---

## Task 4: End-to-end verification and push

**Run:**
- `npx eslint src` — must be 0 errors, 0 warnings.
- `npm run build` — must pass.
- `npm run test` — must pass.
- `cargo clippy -p goble-desktop-tauri --lib -D warnings` — must pass.
- `cargo test -p goble-desktop-tauri --lib` — must pass.
- `cargo test -p goble-core` — must pass.

**Then:** commit and push to `feature/agent-guide-ui`.


---

## Verification Results

Run on branch `feature/agent-guide-ui`:

- `npx eslint src` ✅ exit 0, no errors, no warnings
- `npm run build` ✅ exit 0
- `npm run test` ✅ 11 passed
- `cargo clippy -p goble-desktop-tauri --lib -D warnings` ✅ exit 0
- `cargo test -p goble-desktop-tauri --lib` ✅ 23 passed
- `cargo test -p goble-core` ✅ 149 passed

All commits pushed to `feature/agent-guide-ui`.
