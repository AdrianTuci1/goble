# 09 — Mobile client (deferred)

**Status:** `[ ]` deferred — direction recorded
**Owns:** a future Kotlin app (Android/iOS) doing the same thing
**Depends on:** [`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md), [`../03-workspace-model/workspace.md`](../03-workspace-model/workspace.md), [`../08-threads-deferred/README.md`](../08-threads-deferred/README.md)

## Direction

The mobile app runs the **same product** — the same workspace model, routers, harness and threads — but on Android/iOS (Kotlin). It will not run the harness locally; it connects to a workspace.

## Key architectural idea

- **The local computer becomes a worker** (via the relay, or via Tailscale) so the mobile app can reach a workspace it owns. The relay and its pairing design live in [`../11-relay-and-hub/relay-auth.md`](../11-relay-and-hub/relay-auth.md); the phone should not need a VPN.
- A **worker == a workspace**: it hosts the harness, crons, secrets and the thread-messaging server. So the mobile client connects to a worker and drives a workspace, exactly as the desktop app drives a remote workspace today.
- The mobile client is therefore the same "client" role the desktop app already plays for a remote workspace — the desktop's routing/client transport is the blueprint.
- **The shell on mobile is the terminal, rendered natively.** Paseo's approach is worth copying at the architecture level: the device keeps an `@xterm/headless` buffer in JS and a native view draws each row (CoreText on iOS, Canvas on Android), so Unicode cluster widths, cursor and selection come from xterm's columns while rendering stays fast. Our `goble-web` already owns the parser and the grid; the mobile client needs a native view over the same row model, not a WebView.
- The daemon must replay the PTY at the client's measured size. Paseo's regression there is explicit: the pane attaches with the native view's grid size, so the shell redraws for the phone instead of the desktop.

## Requirements this imposes now (so we don't backtrack)

- Keep the client/workspace contract **transport-agnostic** (mTLS WebSocket), so a Tailscale-addressable worker is reachable from mobile.
- Keep the renderer-agnostic event stream (see [`../06-renderer/README.md`](../06-renderer/README.md)); the mobile client subscribes to the same stream, not to desktop-specific internals.
- Keep routing decisions (local/remote) separate from the transport, so a mobile client only ever needs the remote path.

## Non-goals

- No mobile build in this milestone. This doc only records the forward-compatibility constraints.

## Tasks (when this is picked up)

- [ ] Wire the local machine as a Tailscale-exposed worker (self-as-worker target).
- [ ] Confirm the client/workspace + harness event contracts are mobile-friendly (transport-agnostic).
- [ ] Scaffold the Kotlin client against the same workspace/routing contract.
