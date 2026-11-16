import { useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import {
  EmptyState,
  SectionHeader,
  StatusPill,
  connection,
  palette,
  radius,
  space,
  typography,
  useConnection,
  type ConnectionPhase,
  type ConnectionSnapshot,
  type ConnectionState,
  type RelayStatus,
} from "@goble/common";

import { Sidebar } from "../components/sidebar";
import { TerminalPane } from "../components/terminal-pane";

const phaseLabel: Record<ConnectionPhase, string> = {
  demo: "Demo workspace",
  pairing: "Pairing",
  connected: "Connected",
  error: "Error",
};

const linkLabel: Record<RelayStatus, string> = {
  idle: "Idle",
  connecting: "Connecting",
  live: "Live",
  lost: "Lost",
};

function badge(
  phase: ConnectionPhase,
  status: RelayStatus,
): { label: string; state: ConnectionState } {
  if (phase === "demo") return { label: "Demo workspace", state: "demo" };
  if (phase === "pairing") return { label: "Pairing", state: "demo" };
  if (phase === "error") return { label: "Gateway error", state: "offline" };
  switch (status) {
    case "live":
      return { label: "Gateway live", state: "online" };
    case "lost":
      return { label: "Gateway lost", state: "offline" };
    case "connecting":
      return { label: "Connecting", state: "demo" };
    case "idle":
      return { label: "Gateway paired", state: "demo" };
  }
}

function emptyBody(remote: ConnectionSnapshot): string {
  if (remote.phase === "demo") {
    return "Pair a gateway to connect a worker and open a session in this pane.";
  }
  if (remote.phase === "error") {
    return "Fix the gateway connection above to open a session in this pane.";
  }
  if (remote.workers.length === 0) {
    return "No worker is connected to this gateway yet.";
  }
  return "Sessions opened through the gateway will appear in this pane.";
}

export default function WebShell() {
  const remote = useConnection();
  const status = badge(remote.phase, remote.status);
  const [shellOpen, setShellOpen] = useState(false);
  /** A shell is one worker's child; with none selected there is nothing to open. */
  const canShell = remote.workerId !== null;

  return (
    <SafeAreaView
      edges={["top", "bottom", "left", "right"]}
      style={styles.safe}
    >
      <View style={styles.shell}>
        <Sidebar
          canShell={canShell}
          label={status.label}
          onNewShell={() => setShellOpen(true)}
          onShowSessions={() => setShellOpen(false)}
          server={remote.server}
          shellOpen={shellOpen}
          state={status.state}
        />
        <View style={styles.main}>
          <View style={styles.header}>
            <View style={styles.headerText}>
              <Text style={styles.title}>
                {shellOpen ? "Shell" : "Sessions"}
              </Text>
              <Text style={styles.subtitle}>
                {shellOpen
                  ? (remote.workerId ?? "No worker connected")
                  : "Goble desktop shell"}
              </Text>
            </View>
            <StatusPill label={status.label} state={status.state} />
          </View>

          {shellOpen ? (
            <TerminalPane />
          ) : (
            <ScrollView
              contentContainerStyle={styles.content}
              showsVerticalScrollIndicator={false}
            >
              <View style={styles.card}>
                <SectionHeader title="Connection" />
                <DetailRow label="Phase" value={phaseLabel[remote.phase]} />
                <DetailRow
                  label="Gateway"
                  value={remote.server ?? "None paired"}
                />
                <DetailRow
                  label="Worker"
                  value={remote.workerId ?? "None selected"}
                />
                <DetailRow label="Link" value={linkLabel[remote.status]} />
                <DetailRow label="Workers" value={String(remote.workers.length)} />
                {remote.error ? (
                  <Text style={styles.error}>{remote.error}</Text>
                ) : null}
              </View>

              {remote.workers.length > 0 ? (
                <View style={styles.card}>
                  <SectionHeader
                    title={`Workers · ${remote.workers.length}`}
                  />
                  <View style={styles.workers}>
                    {remote.workers.map((worker) => {
                      const active = worker.id === remote.workerId;
                      return (
                        <Pressable
                          accessibilityRole="button"
                          accessibilityState={{ selected: active }}
                          key={worker.id}
                          onPress={() => void connection.selectWorker(worker.id)}
                          style={({ pressed }) => [
                            styles.workerRow,
                            active && styles.workerRowActive,
                            pressed && styles.pressed,
                          ]}
                        >
                          <Text numberOfLines={1} style={styles.workerId}>
                            {worker.id}
                          </Text>
                          <Text
                            style={[
                              styles.workerMeta,
                              active && styles.workerMetaActive,
                            ]}
                          >
                            {active ? "Selected" : "Connect"}
                          </Text>
                        </Pressable>
                      );
                    })}
                  </View>
                </View>
              ) : null}

              <EmptyState
                body={emptyBody(remote)}
                icon="chatbubble-ellipses-outline"
                title="No session selected"
              />
            </ScrollView>
          )}
        </View>
      </View>
    </SafeAreaView>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <View style={styles.detailRow}>
      <Text style={styles.detailLabel}>{label}</Text>
      <Text numberOfLines={1} style={styles.detailValue}>
        {value}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  safe: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  shell: {
    flex: 1,
    flexDirection: "row",
  },
  main: {
    flex: 1,
  },
  header: {
    alignItems: "center",
    borderBottomColor: palette.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    justifyContent: "space-between",
    paddingHorizontal: space.xl,
    paddingVertical: space.lg,
  },
  headerText: {
    gap: 2,
  },
  title: {
    ...typography.title,
    color: palette.text,
  },
  subtitle: {
    ...typography.caption,
    color: palette.subtle,
  },
  content: {
    alignSelf: "center",
    gap: space.xl,
    maxWidth: 960,
    padding: space.xl,
    width: "100%",
  },
  card: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    padding: space.lg,
  },
  detailRow: {
    alignItems: "center",
    borderTopColor: palette.border,
    borderTopWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.lg,
    justifyContent: "space-between",
    paddingVertical: 9,
  },
  detailLabel: {
    ...typography.caption,
    color: palette.muted,
  },
  detailValue: {
    ...typography.mono,
    color: palette.text,
    flexShrink: 1,
    fontSize: 11,
    textAlign: "right",
  },
  error: {
    ...typography.caption,
    color: palette.error,
    paddingTop: space.sm,
  },
  workers: {
    gap: 2,
  },
  workerRow: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    justifyContent: "space-between",
    paddingHorizontal: space.sm,
    paddingVertical: 7,
  },
  workerRowActive: {
    backgroundColor: palette.raised,
  },
  workerId: {
    ...typography.mono,
    color: palette.muted,
    flexShrink: 1,
    fontSize: 11,
  },
  workerMeta: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
  },
  workerMetaActive: {
    color: palette.accent,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.65,
  },
});
