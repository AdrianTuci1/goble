# 12 — Client apps: `common` / `web` / `mobile`

**Status:** `[~]` in progress — wave 1 landed, waves 2–5 pending
**Owns:** the TypeScript client: the shared core and the two shells over it
**Supersedes:** the Rust/WASM browser client (`crates/goble-web`), marked stale on 2026-11-15

## Decision

The product has **one** client codebase, not two. The phone app built in
[`apps/mobile`](../../apps/mobile) already is the browser surface — the Expo app
exports for web and `react-native-web` is a dependency — so the browser is a
second *shell*, not a second implementation.

- `apps/common` — `@goble/common`: the remote protocol, state, formatting and
  every screen. Platform-neutral by rule.
- `apps/mobile` — the phone shell: expo-router drawer, native modules.
- `apps/web` — the browser shell: desktop layout, `*.web.ts` implementations.
- `crates/goble-web` — **removed**. Its `/app/` URL stays; the gateway serves the
  Expo web export there instead of a wasm bundle.

`crates/goble-ui`, `crates/goble-desktop-*` and `app/` are the **native desktop
app** and are out of scope: that surface is Rust and stays Rust.

## Waves

- [x] **V1 — the workspace skeleton.** Root `package.json` with `apps/*` as npm
  workspaces and a single root `package-lock.json`; `apps/common` as
  `@goble/common` consumed as TS source (`main`/`types` → `src/index.ts`, no
  `dist/`); `apps/mobile/metro.config.js` with `watchFolders` + `nodeModulesPaths`
  so Metro follows the workspace link; the platform interface set in
  `src/platform/index.ts`; a `js / typecheck` CI job (+ `apps/**` in the path
  filters, which previously could not trigger anything).
- [ ] **V2 — extract the shared core.** Move `lib/remote/*`, `theme.ts`,
  `format.ts`, `types.ts`, `threads.ts`, the hooks and the platform-neutral
  components out of `apps/mobile` into `apps/common`, behind the `@common/*`
  import. Every `expo-*` import becomes a `Platform` interface implemented by the
  shell. `tsc --noEmit` must stay green per move.
- [ ] **V3 — `apps/web`.** The Expo web shell over `@goble/common` with a desktop
  layout (permanent sidebar instead of a drawer, keyboard shortcuts, wide
  composition), no native modules; `expo export --platform web` produces the
  bundle.
- [ ] **V4 — cut `crates/goble-web`.** Delete the crate and `scripts/build-web.sh`;
  the gateway keeps serving `/app/` from `crates/goble-gateway/web/`, now filled
  by the Expo export; rewrite the "no bundle" page in `web.rs` and the two tests
  that assert the wasm build command (`web.rs`, `tests/gateway.rs`); update
  `docs/remote-sandboxes.md` and `.agents/11-relay-and-hub/*`.
- [ ] **V5 — gates and docs.** CI builds the web export and fails on a size
  regression; `docs/PROJECT_STRUCTURE.md` gains an `apps/` section;
  `.agents/09-mobile-deferred/README.md` (still describing a Kotlin app) is
  rewritten or folded into this doc.

## Verified so far

`npm install` at the root links `node_modules/@goble/common` → `apps/common`;
`npm run check --workspaces --if-present` is green for `@goble/common` and
`@goble/mobile`; `npm ci --dry-run` accepts the root lockfile.
