import { StyleSheet, Text, View } from "react-native";

import { PageHeader } from "../../components/page-header";
import { Screen } from "../../components/screen";
import { StatusSquare } from "../../components/status-square";
import { naturalSchedule, relativeTime, until } from "../../lib/format";
import { demoSchedules } from "../../lib/mock";
import { palette, space, typography } from "../../lib/theme";

export default function ScheduledScreen() {
  const schedules = demoSchedules();

  return (
    <Screen>
      <PageHeader
        eyebrow="WORKSPACE"
        right={<Text style={styles.count}>{schedules.length}</Text>}
        title="Scheduled"
      />

      <Text style={styles.intro}>
        Workflows the gateway runs without a session. Paused schedules stay
        listed here until you resume them.
      </Text>

      <Text style={styles.sectionLabel}>SCHEDULES</Text>
      <View style={styles.rows}>
        {schedules.map((task) => {
          const active = task.status === "active";
          return (
            <View key={task.id} style={styles.row}>
              <StatusSquare
                color={active ? palette.success : palette.warning}
              />
              <View style={styles.rowText}>
                <View style={styles.titleRow}>
                  <Text numberOfLines={1} style={styles.name}>
                    {task.name}
                  </Text>
                  <Text
                    style={[
                      styles.state,
                      { color: active ? palette.success : palette.warning },
                    ]}
                  >
                    {active ? "ACTIVE" : "PAUSED"}
                  </Text>
                </View>
                <Text numberOfLines={1} style={styles.detail}>
                  {naturalSchedule(task.schedule)}
                </Text>
                <Text style={styles.meta}>
                  next {until(task.nextRun)}
                  {task.lastRun ? ` · last ${relativeTime(task.lastRun)}` : ""}
                </Text>
              </View>
            </View>
          );
        })}
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  count: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 12,
  },
  intro: {
    ...typography.caption,
    color: palette.muted,
    lineHeight: 18,
    marginBottom: space.xl,
    marginTop: -space.sm,
    maxWidth: 330,
  },
  sectionLabel: {
    ...typography.eyebrow,
    color: palette.muted,
    marginBottom: space.sm,
    paddingLeft: space.sm,
  },
  rows: {
    gap: 2,
  },
  row: {
    alignItems: "center",
    borderRadius: 12,
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.sm,
    paddingVertical: 10,
  },
  rowText: {
    flex: 1,
    gap: 2,
  },
  titleRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
  },
  name: {
    ...typography.body,
    color: palette.text,
    flex: 1,
    fontSize: 14,
  },
  state: {
    ...typography.mono,
    fontSize: 9,
    letterSpacing: 0.4,
  },
  detail: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
    lineHeight: 17,
  },
  meta: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
    marginTop: 2,
  },
});
