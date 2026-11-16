import { useEffect, useState } from "react";

import {
  SettingsPage,
  SettingsRow,
  SettingsSection,
  duration,
  relativeTime,
  requestExecutions,
  useConnection,
  type RemoteExecution,
} from "@goble/common";

/** How long a finished execution ran, or null while it is still going. */
function runDuration(execution: RemoteExecution): string | null {
  if (!execution.finished_at) return null;
  const started = Date.parse(execution.started_at);
  const finished = Date.parse(execution.finished_at);
  if (Number.isNaN(started) || Number.isNaN(finished)) return null;
  return duration(Math.max(0, Math.round((finished - started) / 1000)));
}

/** Tint the worker's own status word; the word itself is never rewritten. */
function statusTone(
  status: string,
): "muted" | "success" | "warning" | "error" {
  const value = status.toLowerCase();
  if (/(fail|error|cancel)/.test(value)) return "error";
  if (/(run|start|pending|queue)/.test(value)) return "warning";
  if (/(success|done|complete|finish)/.test(value)) return "success";
  return "muted";
}

/**
 * The newest executions the worker ran.
 *
 * The rows are read-only because the protocol carries no run action; each one
 * reports the worker's status word rather than inventing a friendlier state.
 */
export default function RunsScreen() {
  const remote = useConnection();
  const live = remote.status === "live";
  const [executions, setExecutions] = useState<RemoteExecution[]>([]);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!live) {
      setExecutions([]);
      setError(null);
      return;
    }
    let cancelled = false;
    requestExecutions()
      .then((items) => {
        if (cancelled) return;
        setExecutions(
          [...items].sort((a, b) =>
            b.started_at.localeCompare(a.started_at),
          ),
        );
        setError(null);
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setError(
          reason instanceof Error ? reason.message : "Could not read runs",
        );
      });
    return () => {
      cancelled = true;
    };
  }, [live, reload]);

  return (
    <SettingsPage title="Runs">
      <SettingsSection
        footer="Each run keeps the status the worker reports; this list is read-only."
        title="Runs"
      >
        {error ? (
          <SettingsRow
            chevron={false}
            description={error}
            detail="Retry"
            icon="alert-circle-outline"
            label="Could not read runs"
            onPress={() => setReload((value) => value + 1)}
          />
        ) : null}
        {!error && !live ? (
          <SettingsRow
            chevron={false}
            description="Executions the worker runs appear here. Pair a gateway in Settings to read them."
            icon="cloud-offline-outline"
            label="No gateway connected"
          />
        ) : null}
        {!error && live && executions.length === 0 ? (
          <SettingsRow
            chevron={false}
            description="An execution the worker runs appears here with its status and timing."
            icon="pulse-outline"
            label="No runs yet"
          />
        ) : null}
        {live
          ? executions.map((execution) => {
              const agent = execution.agent_id
                ? `Agent ${execution.agent_id}`
                : execution.worker_id
                  ? `Worker ${execution.worker_id}`
                  : "No agent";
              const ran = runDuration(execution);
              return (
                <SettingsRow
                  chevron={false}
                  description={`${agent} · started ${relativeTime(
                    execution.started_at,
                  )}${ran ? ` · ran ${ran}` : ""}${
                    execution.finished_at
                      ? ` · finished ${relativeTime(execution.finished_at)}`
                      : ""
                  }`}
                  detail={execution.status}
                  detailTone={statusTone(execution.status)}
                  icon="pulse-outline"
                  key={execution.id}
                  label={`Run ${execution.id}`}
                />
              );
            })
          : null}
      </SettingsSection>
    </SettingsPage>
  );
}
