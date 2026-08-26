# 10 — Performance budget

**Status:** `[ ]` budget not yet measured
**Owns:** the perf envelope and how we measure it
**Depends on:** [`README.md`](README.md), [`../06-renderer/renderer-architecture.md`](../06-renderer/renderer-architecture.md)

## Commitments

| Aspect | Target |
| --- | --- |
| Frame loop | keep the frame budget clear even while streaming LLM/terminal output |
| Scrolling | smooth with many messages; virtualization for long transcripts |
| Atals/text | no per-frame glyph layout in the hot path |
| Memory | bounded; compaction keeps the transcript from growing unbounded |
| Startup | acceptable cold start; deferred heavy init off the first frame |

## Approach

- Measure, don't assume: add a debug overlay + frame-time histogram while the shell runs.
- Keep the **hot path** (rebuild + paint) allocation-light; move text/icon layout work to atlas/retained structures.
- Streaming content (token deltas, remote PTY frames) must be decoded on a **worker thread** and presented on the main thread, so a slow link never blocks the frame loop (see [`../06-renderer/remote-terminal-renderer.md`](../06-renderer/remote-terminal-renderer.md)).
- Long conversations rely on **compaction** (see [`../04-agent-runtime/agent-state-and-compaction.md`](../04-agent-runtime/agent-state-and-compaction.md)) to bound memory and render work.

## Verification

- A frame-time histogram under a throttled CPU, with streaming enabled.
- Scroll a long transcript and confirm no frame-time spike.

## Tasks

- [ ] Add a frame-time debug overlay + histogram.
- [ ] Virtualize long message lists / transcripts.
- [ ] Move streaming decode off the main thread and confirm frame budget holds.
