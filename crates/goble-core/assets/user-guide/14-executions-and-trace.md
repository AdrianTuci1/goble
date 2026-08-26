# Executions and Trace

Every turn an agent takes produces an **execution** — a record of the run with its events. Goble lets you follow executions and inspect the trace, both locally and across remote runs.

---

## The Trace

Each execution captures:

- The goal the agent was given.
- The **reasoning** steps it took.
- Every **tool call** — command, arguments, result — emitted as events.
- The **assistant messages** produced by the turn.

The local harness and remote workers feed the **same trace path**, so a run looks the same whether it happened here or on a worker.

## Events

Events flow into app state: `chat:updated`, `chat:ask_user`, `chat:mission`, `chat:turn_finished`, plus `executions:updated` and `agent:*` events. The renderer drains them to keep the view live and streaming deltas arrive in real time.

## The Executions View

Goble has a per-execution trace view and an executions list. From the list you can open any execution and walk its trace — see which commands ran, what changed, and why.

## Logs

Logs are available per execution, worker and level. All log lines are run through the **secret sanitizer** before they are displayed or emitted, so a credential value can't leak into a trace.

---

## Related

- [Tools](09-tools.md) — tool calls that appear in a trace.
- [Monitoring Usage](18-monitoring-usage.md) — dashboards and usage over time.
