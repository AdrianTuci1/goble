# 12 — Client apps: `common` / `web` / `mobile`

**Status:** `[x]` landed — waves 1–4 done; V5 done except the bundle-size gate
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
- [x] **V2 — extract the shared core.** `lib/remote/*` (protocol, socket,
  gateway, connection, entities, sessions, attachments, transcribe), `theme.ts`,
  `format.ts`, `types.ts`, `threads.ts`, `use-models.ts`, `harness.ts`, `mock.ts`,
  `navigation.ts` and `pending-attachment.ts` moved to `apps/common/src/lib/`,
  behind the `@goble/common` import. Two seams were cut out of mobile-only code
  so the move stayed honest: `server-url.ts` and `session-store.ts` (persistence
  through the platform registry), and `model-preferences.ts`. The runtime
  registry (`configurePlatform` / `getPlatformStorage` / `getPlatformHaptics`)
  is what a shell fills in at boot; defaults are `localStorage` when present and
  no-ops otherwise. `apps/mobile/lib/platform.ts` hands it `expo-secure-store`
  and `expo-haptics`. Left in the shell because they need a native module or the
  router: `api.ts`, `attachments.ts`, `notifications.ts`, `preferences.ts`,
  `recorder.ts`, `voice.ts`, `dictation.ts`, `use-voice.ts`, `haptics.ts`,
  `use-workspace.ts`.
- [x] **V3 — `apps/web`.** An Expo-web-only shell over `@goble/common`: a
  permanent 280px sidebar, a header with a live connection `StatusPill`, the
  gateway/worker state read from the shared remote client, and an honest
  `EmptyState` pane ("no session selected") instead of mock conversations. Still
  missing, next: rendering a session/terminal in the pane, keyboard shortcuts and
  a wide composition beyond the sidebar layout.
- [x] **V4 — cut `crates/goble-web`.** The crate, `scripts/build-web.sh` and the
  checked-in `crates/goble-gateway/web/` bundle are gone; the gateway's `/app/`
  route stays and serves whatever `--web-dir` points at, with the fallback page
  in `web.rs` now naming `npm ci` + `npm run web:build` and `apps/web/dist`. The
  two tests that asserted the wasm command (`web.rs`, `tests/gateway.rs`) assert
  the new text.
- [~] **V5 — gates and docs.** The `js / typecheck + web build` CI job runs
  `npm ci`, `npm run typecheck` and `npm run web:build` on Node 24 and uploads
  `apps/web/dist`; `docs/PROJECT_STRUCTURE.md` gained an `apps/` section and
  `.agents/09-mobile-deferred/README.md` no longer describes a Kotlin app.
  **Open:** the size-regression gate — CI uploads the export but does not fail on
  a bundle that grows.

## Verified

`npm ci` at the root installs one hoisted tree (`@goble/common` linked from
`node_modules`); `npm run typecheck` is green for `@goble/common`, `@goble/mobile`
and `@goble/web`; `npm run web:build` exports `apps/web/dist` (entry ≈1.1 MB,
shared chunk ≈630 KB); `npx expo export --platform web` still exports the phone
shell. `cargo` was **not** run for V4 — there is no Rust toolchain in this
environment, so the gateway edits are verified by reading and grep, not by
`cargo test -p goble-gateway`.
