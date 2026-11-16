import { useEffect, useState } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";

import {
  SettingsPage,
  SettingsRow,
  SettingsSection,
  Sheet,
  naturalSchedule,
  relativeTime,
  until,
  demoSchedules,
  useConnection,
  cancelScheduledTask,
  requestScheduledTasks,
  palette,
  radius,
  space,
  type RemoteSchedule,
} from "@goble/common";
import { tapFeedback } from "../../lib/haptics";

/**
 * A `Trigger` is externally tagged: the bare word `Manual`, or one object that
 * names a `Cron`, an `Http` path or a `Heartbeat` interval.
 */
export function describeTrigger(trigger: unknown): string {
  if (trigger === "Manual" || trigger === undefined || trigger === null) {
    return "Manual";
  }
  if (typeof trigger !== "object") return String(trigger);
  const entries = Object.entries(trigger as Record<string, unknown>);
  const [kind, value] = entries[0] ?? [];
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

/**
 * The routines the worker runs without a session.
 *
 * Behind a gateway the list is the worker's scheduler, read with
 * `list_scheduled_tasks`; a routine can be cancelled here, which is the one
 * action the worker protocol carries today. Without a gateway the page shows
 * the demo workspace's routines so the layout stays reviewable.
 */
export default function ScheduledScreen() {
  const remote = useConnection();
  const live = remote.status === "live";
  const [tasks, setTasks] = useState<RemoteSchedule[]>([]);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<RemoteSchedule | null>(null);

  useEffect(() => {
    if (!live) {
      setTasks([]);
      setError(null);
      return;
    }
    let cancelled = false;
    requestScheduledTasks()
      .then((items) => {
        if (cancelled) return;
        setTasks(items);
        setError(null);
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setError(
          reason instanceof Error ? reason.message : "Could not read schedules",
        );
      });
    return () => {
      cancelled = true;
    };
  }, [live, reload]);

  function cancel(task: RemoteSchedule) {
    tapFeedback();
    cancelScheduledTask(task.id);
    setConfirming(null);
    setReload((value) => value + 1);
  }

  if (live) {
    return (
      <SettingsPage title="Scheduled">
        <SettingsSection
          footer="Routines the worker runs without a session. A paused routine keeps its place until you resume it."
          title="Schedules"
        >
          {error ? (
            <SettingsRow
              chevron={false}
              description={error}
              detail="Retry"
              icon="alert-circle-outline"
              label="Could not read schedules"
              onPress={() => setReload((value) => value + 1)}
            />
          ) : null}
          {!error && tasks.length === 0 ? (
            <SettingsRow
              chevron={false}
              description="A schedule is created from the desktop, where it names its agent and cadence."
              icon="calendar-outline"
              label="No schedules yet"
            />
          ) : null}
          {tasks.map((task) => (
            <SettingsRow
              description={`Agent ${task.agent_id}`}
              detail={task.enabled ? "Active" : "Paused"}
              detailTone={task.enabled ? "success" : "warning"}
              icon="calendar-outline"
              key={task.id}
              label={describeTrigger(task.trigger)}
              onPress={() => setConfirming(task)}
            />
          ))}
        </SettingsSection>

        <Sheet onClose={() => setConfirming(null)} visible={confirming !== null}>
          <Text style={styles.sheetTitle}>Cancel this schedule?</Text>
          <Text style={styles.sheetBody}>
            {confirming
              ? `${describeTrigger(confirming.trigger)} · agent ${confirming.agent_id}. The routine stops running; a schedule can be created again from the desktop.`
              : ""}
          </Text>
          <Pressable
            accessibilityLabel="Cancel schedule"
            accessibilityRole="button"
            onPress={() => confirming && cancel(confirming)}
            style={({ pressed }) => [styles.danger, pressed && styles.pressed]}
          >
            <Text style={styles.dangerLabel}>Cancel schedule</Text>
          </Pressable>
        </Sheet>
      </SettingsPage>
    );
  }

  const schedules = demoSchedules();

  return (
    <SettingsPage title="Scheduled">
      <SettingsSection
        footer="Workflows the gateway runs without a session. Paused schedules stay listed here until you resume them."
        title="Schedules"
      >
        {schedules.map((task) => {
          const active = task.status === "active";
          return (
            <SettingsRow
              chevron={false}
              description={`${naturalSchedule(task.schedule)} · next ${until(
                task.nextRun,
              )}${
                task.lastRun ? ` · last ${relativeTime(task.lastRun)}` : ""
              }`}
              detail={active ? "Active" : "Paused"}
              detailTone={active ? "success" : "warning"}
              icon="calendar-outline"
              key={task.id}
              label={task.name}
            />
          );
        })}
      </SettingsSection>
    </SettingsPage>
  );
}

const styles = StyleSheet.create({
  sheetTitle: {
    color: palette.text,
    fontSize: 22,
    fontWeight: "600",
    lineHeight: 28,
    marginBottom: space.sm,
  },
  sheetBody: {
    color: palette.muted,
    fontSize: 14,
    lineHeight: 20,
    marginBottom: space.xl,
  },
  danger: {
    alignItems: "center",
    backgroundColor: palette.errorSoft,
    borderColor: palette.error,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    justifyContent: "center",
    minHeight: 48,
    paddingHorizontal: 16,
  },
  dangerLabel: {
    color: palette.error,
    fontSize: 16,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.75,
  },
});
