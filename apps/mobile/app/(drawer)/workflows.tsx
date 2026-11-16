import { useEffect, useState } from "react";

import {
  SettingsPage,
  SettingsRow,
  SettingsSection,
  relativeTime,
  requestWorkflows,
  useConnection,
  type RemoteWorkflow,
} from "@goble/common";

/**
 * The workflows the worker stores.
 *
 * The worker protocol carries no workflow action, so the rows are read-only:
 * there is no chevron and nothing pretends a tap would start or pause one.
 */
export default function WorkflowsScreen() {
  const remote = useConnection();
  const live = remote.status === "live";
  const [workflows, setWorkflows] = useState<RemoteWorkflow[]>([]);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!live) {
      setWorkflows([]);
      setError(null);
      return;
    }
    let cancelled = false;
    requestWorkflows()
      .then((items) => {
        if (cancelled) return;
        setWorkflows(
          [...items].sort((a, b) => b.updated_at.localeCompare(a.updated_at)),
        );
        setError(null);
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setError(
          reason instanceof Error ? reason.message : "Could not read workflows",
        );
      });
    return () => {
      cancelled = true;
    };
  }, [live, reload]);

  return (
    <SettingsPage title="Workflows">
      <SettingsSection
        footer="A workflow runs on the worker; this list is read-only."
        title="Workflows"
      >
        {error ? (
          <SettingsRow
            chevron={false}
            description={error}
            detail="Retry"
            icon="alert-circle-outline"
            label="Could not read workflows"
            onPress={() => setReload((value) => value + 1)}
          />
        ) : null}
        {!error && !live ? (
          <SettingsRow
            chevron={false}
            description="Workflows the worker stores appear here. Pair a gateway in Settings to read them."
            icon="cloud-offline-outline"
            label="No gateway connected"
          />
        ) : null}
        {!error && live && workflows.length === 0 ? (
          <SettingsRow
            chevron={false}
            description="A workflow the worker stores appears here with its trigger and whether it is enabled."
            icon="layers-outline"
            label="No workflows yet"
          />
        ) : null}
        {live
          ? workflows.map((workflow) => {
              const description =
                workflow.description.trim() ||
                workflow.trigger.trim() ||
                "No description";
              return (
                <SettingsRow
                  chevron={false}
                  description={`${description} · updated ${relativeTime(
                    workflow.updated_at,
                  )}`}
                  detail={workflow.enabled ? "Enabled" : "Paused"}
                  detailTone={workflow.enabled ? "success" : "warning"}
                  icon="layers-outline"
                  key={workflow.id}
                  label={workflow.name}
                />
              );
            })
          : null}
      </SettingsSection>
    </SettingsPage>
  );
}
