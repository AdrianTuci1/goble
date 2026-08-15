# Goble Final Polish Plan

**Goal:** Close the remaining end-to-end gaps in chat runtime, clean up lint errors, and consolidate redundant pages so the app is cohesive.

**Branch:** `feature/agent-guide-ui`

---

## 1 — ChatArea invokes agents on mention

### Objective
In `ChatArea`, when a user sends a message that mentions an agent, the agent should be invoked via `runAgentForThreadReply` and the UI should show loading feedback and a link to the execution trace.

### Files to touch
- `crates/goble-desktop/src/components/ChatArea.tsx`
- `crates/goble-desktop/src/components/ChatArea.css`
- `crates/goble-desktop/src/stores/appStore.ts` (if needed for trace state)
- `crates/goble-desktop/src/tauri/api.ts` (already exports `runAgentForThreadReply`)

### Implementation
1. After `postThreadMessage` succeeds, detect agent mentions in the sent text using `extractMentions`.
2. For each agent mention, call `runAgentForThreadReply(runtimeTarget, activeThreadId, agentId, text)`.
3. Show a loading indicator in the chat (already exists) while any agent run is pending.
4. Store the first pending `trace_id` locally so a "Trace" link can be shown near the loading indicator.
5. When the agent reply arrives via `thread:messages:updated`, remove the loading indicator.

### Verification
- Send a message with an agent mention in a thread chat; the agent is invoked (worker reachable or not, no crash).
- Build passes and eslint errors in ChatArea are fixed.

### Commit
`feat(chat): invoke agents on mention and show trace link`

---

## 2 — Fix all eslint errors

### Objective
`npx eslint src` currently reports errors. Clean them up so the codebase is lint-clean.

### Files to touch
- `crates/goble-desktop/src/App.tsx`
- `crates/goble-desktop/src/__mocks__/tauri-core.ts`
- `crates/goble-desktop/src/__mocks__/tauri-event.ts`
- `crates/goble-desktop/src/components/ChatArea.tsx`
- `crates/goble-desktop/src/components/ComposerRuntimeSelector.tsx`
- `crates/goble-desktop/src/components/RightSidebar.tsx`
- `crates/goble-desktop/src/pages/AgentTracePage.tsx`
- `crates/goble-desktop/src/pages/SettingsPage.tsx`

### Implementation
1. `App.tsx`: use `const unsubs` and either add missing dependencies to `useEffect` or suppress with `// eslint-disable-next-line react-hooks/exhaustive-deps` (prefer adding dependencies).
2. Mocks: prefix unused parameters with `_` and mark them as intentionally unused.
3. `ChatArea.tsx`: `const unsubs`, remove unused `e` catch variable.
4. `ComposerRuntimeSelector.tsx`: move exported non-component helper (`runtimeTargetLabel`) to a separate file or export only components.
5. `RightSidebar.tsx`: replace `any` with proper type.
6. `AgentTracePage.tsx`: replace `any` with proper type.
7. `SettingsPage.tsx`: move profile field initialization out of effect (derive initial state from `profile` directly).

### Verification
- `npx eslint src` exits with 0 errors and 0 warnings.
- `npm run build` passes.
- `npm run test` passes.

### Commit
`chore: fix all eslint errors`

---

## 3 — Consolidate redundant pages

### Objective
`ExecutionsPage` is now redundant because `AgentTracePage` is richer. `KnowledgePage` is just a log dump. Clean them up.

### Files to touch
- `crates/goble-desktop/src/pages/ExecutionsPage.tsx`
- `crates/goble-desktop/src/App.tsx`
- `crates/goble-desktop/src/components/Sidebar.tsx`
- `crates/goble-desktop/src/pages/KnowledgePage.tsx` (optional)

### Implementation
1. Replace `ExecutionsPage` content with a redirect or a thin wrapper that renders `AgentTracePage`.
2. Remove `/executions` route from `App.tsx` or make it redirect to `/traces`.
3. Remove Executions icon from sidebar or make it navigate to `/traces`.
4. Keep `KnowledgePage` but rename it to "Logs" and make it a simple log viewer (already is). Optionally rename route to `/logs` and sidebar label.

### Verification
- Sidebar has no broken route.
- `/executions` redirects or shows traces.
- Build and tests pass.

### Commit
`chore: consolidate executions page into traces, rename knowledge to logs`

---

## 4 — End-to-end verification and final push

### Verification commands
```bash
cd crates/goble-desktop
npx eslint src
npm run build
npm run test

cd crates/goble-desktop/src-tauri
cargo check -p goble-desktop-tauri --all-targets
cargo test -p goble-desktop-tauri --lib

cd /root/goble
cargo test -p goble-core
```

### Final commit
`chore: verify final polish plan`

### Push
Push all commits to `feature/agent-guide-ui`.

---

## Notes
- Keep changes focused on one concern per commit.
- Do not add new placeholder UI; replace existing stubs with working behavior.
- No new backend features are needed for this plan; it is frontend consolidation.


---

## Verification Results

Run on branch `feature/agent-guide-ui`:

- `npx eslint src` ✅ exit 0, no errors, no warnings
- `npm run build` ✅ exit 0
- `npm run test` ✅ 11 passed
- `cargo check -p goble-desktop-tauri --all-targets` ✅ exit 0, no warnings
- `cargo test -p goble-desktop-tauri --lib` ✅ 23 passed
- `cargo test -p goble-core` ✅ 149 passed

All commits pushed to `feature/agent-guide-ui`.
