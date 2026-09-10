# 05 — Runtime targets

**Status:** `[~]` partial (local unsupported, self-as-worker absent)
**Owns:** the enumerable set of places the harness can run
**Depends on:** [`README.md`](README.md)

## The targets

```rust
enum RuntimeTarget {
    Local,                             // in-process / local worker
    Worker { worker_id: WorkerId },    // remote goblin worker
    SelfAsWorker,                      // this machine as a worker (Tailscale) for mobile
}
```

| Target | Where the harness runs | Connection | Renderer |
| --- | --- | --- | --- |
| `Local` | this machine, in-app or local worker | none / loopback | in-process |
| `Worker { .. }` | a remote goblin worker | WebSocket over mTLS | streamed events (remote-terminal-renderer) |
| `SelfAsWorker` | this machine, exposed via Tailscale | Tailscale | served to a mobile client |

## Addressability

- Targets are resolved from a **workspace** (see [`../03-workspace-model/workspace.md`](../03-workspace-model/workspace.md)): a workspace's `kind` maps to a target.
- A `Worker` target needs a **worker id** and mTLS bundle (existing `WorkerConfig` + `worker_bundle`).

## Selection rules

- If the workspace is `Local` and the machine has the harness → `Local`.
- If the workspace is `Remote` → `Worker { .. }` (resolved via `WorkerPool`, round-robin or tag-first).
- The **local machine as worker** is not a normal path for the desktop UI; it is the future mobile path (self-as-worker via Tailscale), deferred to [`../09-mobile-deferred/README.md`](../09-mobile-deferred/README.md).

## Tasks

- [ ] Add `Local` and `SelfAsWorker` to the runtime-target set (and remove the `local` bail).
- [ ] Represent each target's connection params (local handle / mTLS address / tailscale addr).
- [ ] Map a workspace `kind` → the default target, but allow per-conversation override.
