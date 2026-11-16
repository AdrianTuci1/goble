import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";

import {
  SectionHeader,
  naturalSchedule,
  palette,
  plural,
  radius,
  relativeTime,
  space,
  typography,
  type RemoteSchedule,
  type RemoteSubagent,
} from "@goble/common";

/**
 * The open conversation's subagents above its routines.
 *
 * The children are filtered by the caller: this component draws the rows it
 * receives and never reads the relay itself. A subagent is a stored child chat,
 * so a row opens that transcript; a routine is still read-only.
 *
 * The worker keys a scheduled task by `agent_id` but a stored chat carries no
 * agent id, so the routine list is the worker's whole set — the same set the
 * native right sidebar receives — and each task names its agent on its second
 * line rather than inventing an attribution it cannot read.
 */
export function ChatSidebar({
  subagents,
  subagentsError,
  onOpenSubagent,
  schedules,
  error,
}: {
  subagents: RemoteSubagent[];
  subagentsError: string | null;
  onOpenSubagent: (subagent: RemoteSubagent) => void;
  schedules: RemoteSchedule[];
  error: string | null;
}) {
  return (
    <View style={styles.root}>
      <View style={styles.subagents}>
        <SectionHeader
          title={`Subagents${subagents.length > 0 ? ` · ${subagents.length}` : ""}`}
        />

        {subagents.length === 0 ? (
          <Text style={styles.empty}>No subagents for this conversation.</Text>
        ) : (
          <ScrollView
            contentContainerStyle={styles.list}
            showsVerticalScrollIndicator={false}
            style={styles.subagentScroll}
          >
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
                    styles.subagentRow,
                    pressed && styles.pressed,
                  ]}
                >
                  <Text numberOfLines={1} style={styles.subagentTitle}>
                    {subagent.title || subagent.id}
                  </Text>
                  <Text numberOfLines={2} style={styles.subagentPreview}>
                    {subagent.last_message || "No messages yet."}
                  </Text>
                  <Text numberOfLines={1} style={styles.meta}>
                    {plural(subagent.message_count, "message", "messages")}
                    {last ? ` · ${last}` : ""}
                  </Text>
                </Pressable>
              );
            })}
          </ScrollView>
        )}

        {subagentsError ? (
          <Text style={styles.error}>{subagentsError}</Text>
        ) : null}
      </View>

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
  subagents: {
    marginBottom: space.xl,
  },
  subagentScroll: {
    maxHeight: 220,
  },
  subagentRow: {
    borderRadius: radius.md,
    gap: 1,
    paddingHorizontal: space.sm,
    paddingVertical: 7,
  },
  subagentTitle: {
    ...typography.callout,
    color: palette.text,
    fontSize: 12,
    fontWeight: "500",
    lineHeight: 16,
  },
  subagentPreview: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
    lineHeight: 15,
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
  pressed: {
    opacity: 0.7,
  },
});
