import { ScrollView, StyleSheet, Text, View } from "react-native";

import {
  SectionHeader,
  naturalSchedule,
  palette,
  radius,
  space,
  typography,
  type RemoteSchedule,
} from "@goble/common";

/**
 * The conversation's routines, as the worker's scheduler reports them.
 *
 * The worker keys a scheduled task by `agent_id` but a stored chat carries no
 * agent id, so this draws the worker's whole routine list — the same set the
 * native right sidebar receives — and names each task's agent on its second
 * line rather than inventing an attribution it cannot read.
 */
export function ChatSidebar({
  schedules,
  error,
}: {
  schedules: RemoteSchedule[];
  error: string | null;
}) {
  return (
    <View style={styles.root}>
      <SectionHeader
        title={`Routines${schedules.length > 0 ? ` · ${schedules.length}` : ""}`}
      />

      {schedules.length === 0 ? (
        <Text style={styles.empty}>No routines on this worker.</Text>
      ) : (
        <ScrollView
          contentContainerStyle={styles.list}
          showsVerticalScrollIndicator={false}
          style={styles.scroll}
        >
          {schedules.map((task) => (
            <View key={task.id} style={styles.row}>
              <View
                style={[
                  styles.dot,
                  task.enabled ? styles.dotEnabled : styles.dotPaused,
                ]}
              />
              <View style={styles.text}>
                <Text numberOfLines={2} style={styles.schedule}>
                  {scheduleLabel(task.trigger)}
                </Text>
                <Text numberOfLines={1} style={styles.meta}>
                  {task.enabled ? "Enabled" : "Paused"} · {task.agent_id}
                </Text>
              </View>
            </View>
          ))}
        </ScrollView>
      )}

      {error ? <Text style={styles.error}>{error}</Text> : null}
    </View>
  );
}

/**
 * A `Trigger` is externally tagged: the bare word `Manual`, or one object that
 * names a `Cron`, an `Http` path or a `Heartbeat` interval.
 */
function scheduleLabel(trigger: unknown): string {
  if (trigger === "Manual" || trigger === undefined || trigger === null) {
    return "Manual";
  }
  if (typeof trigger !== "object") return String(trigger);
  const [kind, value] = Object.entries(trigger as Record<string, unknown>)[0] ?? [];
  if (!kind) return "Scheduled";
  const body = (value ?? {}) as Record<string, unknown>;
  if (kind === "Cron" && typeof body.expression === "string") {
    return naturalSchedule(body.expression);
  }
  if (kind === "Http" && typeof body.path === "string") {
    return `Webhook ${body.path}`;
  }
  if (kind === "Heartbeat" && typeof body.interval_seconds === "number") {
    const minutes = Math.round(body.interval_seconds / 60);
    return minutes >= 1
      ? `Every ${minutes} min`
      : `Every ${body.interval_seconds}s`;
  }
  return kind;
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.surface,
    borderLeftColor: palette.border,
    borderLeftWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: space.lg,
    paddingVertical: space.lg,
    width: 280,
  },
  scroll: {
    flex: 1,
  },
  list: {
    gap: 2,
    paddingBottom: space.md,
  },
  row: {
    alignItems: "flex-start",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.sm,
    paddingHorizontal: space.sm,
    paddingVertical: 7,
  },
  dot: {
    borderRadius: 3,
    height: 6,
    marginTop: 5,
    width: 6,
  },
  dotEnabled: {
    backgroundColor: palette.accent,
  },
  dotPaused: {
    backgroundColor: palette.subtle,
  },
  text: {
    flex: 1,
    gap: 1,
  },
  schedule: {
    ...typography.callout,
    color: palette.text,
    fontSize: 12,
    fontWeight: "500",
    lineHeight: 16,
  },
  meta: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
    lineHeight: 14,
  },
  empty: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 12,
    paddingHorizontal: space.sm,
    paddingVertical: space.sm,
  },
  error: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
    paddingHorizontal: space.sm,
    paddingTop: space.sm,
  },
});
