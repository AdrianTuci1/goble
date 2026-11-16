# 12 — Client apps: `common` / `web` / `mobile`

**Status:** `[x]` landed — waves 1–6 done
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
  conversation unless a text field held focus. The wide composition landed in
  this pass: with a conversation open at a viewport width of at least **1100
  px**, `apps/web/app/index.tsx` appends a third 280px column
  (`apps/web/components/chat-sidebar.tsx`) after the conversation pane. It draws
  the routines `useWorkerEntities` already reads with `requestScheduledTasks`
  (`apps/web/lib/worker-entities.ts`): each row shows the trigger spoken through
  `naturalSchedule` (or its webhook/heartbeat form), the task's enabled or
  paused state and its `agent_id`. The column keeps the last good list and
  reports the error beneath it when a read fails, and an honest "No routines on
  this worker." empty state when there are none. Below 1100 px, with no
  conversation open, in the shell and in the sessions overview it is not
  composed, so narrow widths keep the existing two columns. The native **+**
  action is deliberately left out: the web protocol has no schedule-create call,
  so there is nothing to wire and no dead button. The worker keys a routine by
  `agent_id`, but a stored chat carries no agent id, so the column draws the
  worker's whole routine list — the same set the native right sidebar receives —
  and names each row's agent rather than inventing a per-conversation filter.
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
- [x] **V6 — worker entities on both shells.** The worker can finally name a
  subagent: `query_entities` honours its `query` field, `subagents` answers with
  the child chats (`Store::list_subagent_chats`: newest first, ≤100, one SQL read
  with the parent's title, the message count and the last message), and `chat`
  with a query answers under `chat:<id>` so a client can open a child's own
  transcript; an unknown id is an empty list, not an error. The distinct answer
  kind is what pairs two reads in flight, since the relay carries no request id.
  `executions` is capped at 50 and drops the full `trace`. `@goble/common` wraps
  them (`requestSubagents`, `requestWorkflows`, `requestExecutions`,
  `requestChat`). The browser overview gained **Workflows · N / Runs · N /
  Subagents · N** (`apps/web/components/work-cards.tsx`, read by
  `apps/web/lib/worker-work.ts`), the wide third column lists the open
  conversation's children, and a press opens
  `apps/web/components/subagent-pane.tsx`, a read-only transcript with a Back
  bar (Escape returns to the parent; `mod+k` refuses while it is open). The phone
  gained `subagents` / `workflows` / `runs` drawer screens and
  `session/[id].tsx` now falls back to `requestChat(id)`, so a pushed child id
  loads its stored transcript.

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

The wide composition was gated from scratch in this pass: `npm ci` → exit 0;
`npm run typecheck` → exit 0 for `@goble/common`, `@goble/mobile` and
`@goble/web`; `npm run web:build` → exit 0 exporting `apps/web/dist`;
`npm run web:size` → accepted at 1736.1 KB raw / 475.9 KB gzip / 1090.6 KB
largest. **Not verified:** the third column in a browser — this checkout carries
no headless-browser harness and the container has no Chromium binary, so the
breakpoint composition and routine rendering were not exercised beyond
typecheck and the export.

V6 was gated on the integrated branch: `cargo test -p goblin-worker --lib` →
67 passed, 0 failed; `cargo test -p goble-core --lib store::` → 24 passed
(the new tests assert a top-level chat is not listed as a subagent, a child's
messages come back under `chat:<id>`, an unknown id answers nothing, and 55
inserted runs trim to the newest 50 without a `trace` key); `npm run typecheck`
→ exit 0 for `@goble/common`, `@goble/mobile` and `@goble/web`;
`npm run web:build` → exit 0 and `npm run web:size` accepts **1749.8 KB raw /
478.4 KB gzip / 1090.6 KB largest**; `npx expo export --platform web` from
`apps/mobile` exports the phone shell with the three new routes. **Not
verified:** a browser or a phone — this checkout has neither a headless harness
nor a device, so the cards, the drawer entries, the subagent view, scrolling and
the tap targets were not exercised; typecheck and the export prove the types and
that every route bundles, nothing more.
