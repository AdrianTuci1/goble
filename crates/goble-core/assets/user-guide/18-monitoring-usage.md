# Monitoring Usage

Goble surfaces how your agents are being used so you can see activity, follow long-running runs, and keep an eye on cost and behavior.

---

## What You Can Monitor

- **Executions** — every run, its status and its trace (see [Executions and Trace](14-executions-and-trace.md)).
- **Agent activity** — which agents are active, their sub-agent tree and live status.
- **Logs** — filtered by execution, worker and level, with credential values scrubbed (see [Executions and Trace](14-executions-and-trace.md#logs)).
- **Usage over time** — aggregated counts and cost per model/execution.

## The Dashboard

A dashboard aggregates executions and usage: runs in flight, recent failures, and per-agent totals. Per-execution views let you dig from a summary into the exact trace.

## Telemetry

Telemetry is a config setting, not a data-sharing toggle. Where "coding-data sharing" is available, that's a separate, unrelated setting managed by a workspace/team admin.

## Keeping Secrets Out

All log and trace lines pass through the **secret sanitizer** before display or emit, so a credential value can never appear in what you monitor.

---

## Related

- [Executions and Trace](14-executions-and-trace.md) — the per-run detail.
- [Configuration](16-configuration.md) — where telemetry lives.
