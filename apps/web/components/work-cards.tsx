import {
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";

import {
  SectionHeader,
  duration,
  palette,
  plural,
  radius,
  relativeTime,
  space,
  typography,
  type RemoteExecution,
  type RemoteSubagent,
  type RemoteWorkflow,
} from "@goble/common";

/** How long a finished execution ran; null while it is still going. */
function runDuration(execution: RemoteExecution): string | null {
  if (!execution.finished_at) return null;
  const started = Date.parse(execution.started_at);
  const finished = Date.parse(execution.finished_at);
  if (Number.isNaN(started) || Number.isNaN(finished)) return null;
  return duration(Math.max(0, Math.round((finished - started) / 1000)));
}

/** Tint the worker's own status word; the word itself is never rewritten. */
function statusTone(status: string): "muted" | "success" | "warning" | "error" {
  const value = status.toLowerCase();
  if (/(fail|error|cancel)/.test(value)) return "error";
  if (/(run|start|pending|queue)/.test(value)) return "warning";
  if (/(success|done|complete|finish)/.test(value)) return "success";
  return "muted";
}

const statusColor = {
  error: palette.error,
  muted: palette.muted,
  success: palette.success,
  warning: palette.warning,
} as const;

/** The workflows the worker stores. The protocol carries no workflow action. */
export function WorkflowsCard({
  workflows,
  error,
}: {
  workflows: RemoteWorkflow[];
  error: string | null;
}) {
  return (
    <View style={styles.card}>
      <SectionHeader title={`Workflows · ${workflows.length}`} />
      {workflows.length === 0 ? (
        <Text style={styles.empty}>No workflows stored.</Text>
      ) : (
        <View style={styles.rows}>
          {workflows.map((workflow) => {
            const updated = relativeTime(workflow.updated_at);
            const description =
              workflow.description.trim() || workflow.trigger.trim();
            return (
              <View key={workflow.id} style={styles.row}>
                <View style={styles.text}>
                  <Text numberOfLines={1} style={styles.name}>
                    {workflow.name || workflow.id}
                  </Text>
                  {description ? (
                    <Text numberOfLines={2} style={styles.detail}>
                      {description}
                    </Text>
                  ) : null}
                  <Text numberOfLines={1} style={styles.meta}>
                    {workflow.enabled ? "Enabled" : "Paused"}
                    {updated ? ` · Updated ${updated}` : ""}
                  </Text>
                </View>
              </View>
            );
          })}
        </View>
      )}
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

/** The newest executions the worker ran, exactly as their status fields read. */
export function RunsCard({
  executions,
  error,
}: {
  executions: RemoteExecution[];
  error: string | null;
}) {
  return (
    <View style={styles.card}>
      <SectionHeader title={`Runs · ${executions.length}`} />
      {executions.length === 0 ? (
        <Text style={styles.empty}>No runs recorded.</Text>
      ) : (
        <View style={styles.rows}>
          {executions.map((execution) => {
            const started = relativeTime(execution.started_at);
            const finished = execution.finished_at
              ? relativeTime(execution.finished_at)
              : "";
            const ran = runDuration(execution);
            return (
              <View key={execution.id} style={styles.row}>
                <View style={styles.text}>
                  <View style={styles.runHead}>
                    <Text
                      numberOfLines={1}
                      style={[
                        styles.status,
                        { color: statusColor[statusTone(execution.status)] },
                      ]}
                    >
                      {execution.status || "Unknown status"}
                    </Text>
                    <Text numberOfLines={1} style={styles.agent}>
                      {execution.agent_id
                        ? `Agent ${execution.agent_id}`
                        : execution.worker_id
                          ? `Worker ${execution.worker_id}`
                          : "No agent"}
                    </Text>
                  </View>
                  <Text numberOfLines={1} style={styles.meta}>
                    {started ? `Started ${started}` : "Start time unknown"}
                    {ran ? ` · Ran ${ran}` : ""}
                    {finished ? ` · Finished ${finished}` : ""}
                  </Text>
                </View>
              </View>
            );
          })}
        </View>
      )}
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

/** Every child chat the worker stores; a row opens that subagent's transcript. */
export function SubagentsCard({
  subagents,
  error,
  onOpenSubagent,
}: {
  subagents: RemoteSubagent[];
  error: string | null;
  onOpenSubagent: (subagent: RemoteSubagent) => void;
}) {
  return (
    <View style={styles.card}>
      <SectionHeader title={`Subagents · ${subagents.length}`} />
      {subagents.length === 0 ? (
        <Text style={styles.empty}>No subagents stored.</Text>
      ) : (
        <View style={styles.rows}>
          {subagents.map((subagent) => {
            const last = subagent.last_message_at
              ? relativeTime(subagent.last_message_at)
              : "";
            return (
              <Pressable
                accessibilityRole="button"
                key={subagent.id}
                onPress={() => onOpenSubagent(subagent)}
                style={({ pressed }) => [
                  styles.row,
                  styles.subagentRow,
                  pressed && styles.pressed,
                ]}
              >
                <View style={styles.text}>
                  <Text numberOfLines={1} style={styles.name}>
                    {subagent.title || subagent.id}
                  </Text>
                  <Text numberOfLines={1} style={styles.detail}>
                    {subagent.parent_title
                      ? `From ${subagent.parent_title}`
                      : "Parent conversation not stored"}
                  </Text>
                  <Text numberOfLines={1} style={styles.meta}>
                    {plural(subagent.message_count, "message", "messages")}
                    {last ? ` · Last message ${last}` : ""}
                  </Text>
                </View>
              </Pressable>
            );
          })}
        </View>
      )}
      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    padding: space.lg,
  },
  rows: {
    gap: 2,
  },
  row: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    justifyContent: "space-between",
    paddingHorizontal: space.sm,
    paddingVertical: 7,
  },
  subagentRow: {
    alignItems: "flex-start",
  },
  text: {
    flex: 1,
    gap: 1,
  },
  name: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
    fontWeight: "500",
  },
  detail: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
    lineHeight: 15,
  },
  meta: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 11,
  },
  runHead: {
    alignItems: "baseline",
    flexDirection: "row",
    gap: space.sm,
    justifyContent: "space-between",
  },
  status: {
    ...typography.caption,
    flexShrink: 1,
    fontSize: 12,
    fontWeight: "600",
  },
  agent: {
    ...typography.mono,
    color: palette.muted,
    flexShrink: 1,
    fontSize: 11,
  },
  empty: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 12,
  },
  error: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
    paddingTop: space.sm,
  },
  pressed: {
    opacity: 0.65,
  },
});
