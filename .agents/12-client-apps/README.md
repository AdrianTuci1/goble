# 12 — Client apps: `common` / `web` / `mobile`

**Status:** `[x]` landed — waves 1–5 done
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
  permanent 280px sidebar with a **New shell** action, a header with a live
  connection `StatusPill`, the gateway/worker state read from the shared remote
  client, and an honest `EmptyState` pane ("no session selected") instead of
  mock conversations. The pane renders the shared `TerminalSurface` over one pty
  (`useTerminal("web:shell")`), so a shell opened on the selected worker is typed
  into and read back in the browser, with the shared key bar for control keys.
  Served by the gateway at `/app/`, the shell adopts its own origin and the
  `HttpOnly` login cookie (`Connection.adoptBrowserSession`) instead of asking
  for a PIN the page has already answered. The worker's conversations are the
  sidebar's second list (`useWorkerThreads`), and a selected one draws its
  stored transcript with a composer: a message starts a turn (`startTurn`),
  deltas stream into the reply, and a question (quick replies or typed) or a
  proposed command (run / reject) is answered in the pane. The overview gained
  two cards fed by the worker while the relay is live: **Agents · N**
  (`requestAgents`, each agent's name, id and updated age) and **Scheduled · N**
  (`requestScheduledTasks`, each task's agent id and enabled state), each keeping
  its last good list when a read fails. Web-only shortcuts landed in
  `apps/web/lib/shortcuts.ts` (`useShellShortcuts`): `mod+1` returned to the
  sessions overview, `mod+2` opened the shell, `mod+n` opened a new shell,
  `mod+k` focused the open conversation's composer, and Escape left the shell or
  conversation unless a text field held focus. Still missing: the wide
  composition beyond the sidebar + pane.
- [x] **V4 — cut `crates/goble-web`.** The crate, `scripts/build-web.sh` and the
  checked-in `crates/goble-gateway/web/` bundle are gone; the gateway's `/app/`
  route stays and serves whatever `--web-dir` points at, with the fallback page
  in `web.rs` now naming `npm ci` + `npm run web:build` and `apps/web/dist`. The
  two tests that asserted the wasm command (`web.rs`, `tests/gateway.rs`) assert
  the new text. The export is built for that mount — `experiments.baseUrl` is
  `/app` in `apps/web/app.json` — so the index and every bundled asset are
  requested below `/app/` rather than from the domain root, which the gateway
  does not serve.
- [x] **V5 — gates and docs.** The `js / typecheck + web build` CI job runs
  `npm ci`, `npm run typecheck`, `npm run web:build` and `npm run web:size` on
  Node 24, then uploads `apps/web/dist`; `docs/PROJECT_STRUCTURE.md` gained an
  `apps/` section and `.agents/09-mobile-deferred/README.md` no longer describes
  a Kotlin app. The size gate is `scripts/check-web-size.mjs`: it sums the
  export's JavaScript and fails over 1900 KB raw, 520 KB gzip or 1250 KB for the
  largest file — the 2026-11-16 export (≈1734 KB / 475 KB / 1091 KB) plus about
  10% of headroom. A change that needs more raises the number in the same
  commit, where a reviewer sees it move.

## Verified

`npm ci` at the root installs one hoisted tree (`@goble/common` linked from
`node_modules`); `npm run typecheck` is green for `@goble/common`, `@goble/mobile`
and `@goble/web`; `npm run web:build` exports `apps/web/dist` with `/app/`
asset URLs (entry ≈1.1 MB, shared chunk ≈630 KB, 1734 KB of JavaScript / 475 KB
gzipped) and `npm run web:size` accepts it; `npx expo export --platform web`
still exports the phone shell. V4's gateway edits were verified with the Rust
toolchain installed for this pass: `cargo test -p goble-gateway` → 22 passed,
0 failed. The browser shell was loaded from a static `/app/` mount in headless
Chromium with a faked gateway API and relay: it boots, adopts the origin, lists
the worker and its conversations, draws a stored transcript, streams a reply to
a message sent from the composer, answers an `ask_user` question from a quick
reply and approves a proposed command — and the New shell action draws the pty's
output. A later pass over the same export read the faked agent and schedule
lists into their cards, opened the shell with `mod+2` and `mod+n`, returned with
`mod+1`, focused the composer with `mod+k` and left the shell and conversation
with Escape with no console or page error. No real gateway or worker was
involved, and a host without `/api/workers` keeps the demo shell.
