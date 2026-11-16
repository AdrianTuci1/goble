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
  relativeTime,
  space,
  typography,
  threadConversation,
  useConnection,
  useWorkerThreads,
  type ConnectionPhase,
  type ConnectionSnapshot,
  type ConnectionState,
  type RelayStatus,
} from "@goble/common";

import { ConversationPane } from "../components/conversation-pane";
import { Sidebar, type ThreadRow } from "../components/sidebar";
import { TerminalPane } from "../components/terminal-pane";
import { useShellShortcuts } from "../lib/shortcuts";
import { useWorkerEntities } from "../lib/worker-entities";

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
  const chats = useWorkerThreads();
  const entities = useWorkerEntities();
  const [shellOpen, setShellOpen] = useState(false);
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [composerFocusSignal, setComposerFocusSignal] = useState(0);
  /** A shell is one worker's child; with none selected there is nothing to open. */
  const canShell = remote.workerId !== null;
  const activeChat =
    chats.find((thread) => thread.id === activeThreadId) ?? null;
  const conversationOpen = !shellOpen && activeChat !== null;

  function showSessions() {
    setShellOpen(false);
    setActiveThreadId(null);
  }

  function openShell() {
    setShellOpen(true);
  }

  useShellShortcuts({
    canShell,
    shellOpen,
    conversationOpen,
    onClearConversation: () => setActiveThreadId(null),
    onFocusComposer: () => setComposerFocusSignal((value) => value + 1),
    onNewShell: openShell,
    onOpenShell: openShell,
    onShowSessions: showSessions,
  });

  const threads: ThreadRow[] = chats.map((chat) => {
    const thread = threadConversation(chat);
    return { id: thread.id, title: thread.title, preview: thread.preview };
  });

  return (
    <SafeAreaView
      edges={["top", "bottom", "left", "right"]}
      style={styles.safe}
    >
      <View style={styles.shell}>
        <Sidebar
          activeThreadId={activeThreadId}
          canShell={canShell}
          label={status.label}
          onNewShell={openShell}
          onSelectThread={(id) => {
            setShellOpen(false);
            setActiveThreadId(id);
          }}
          onShowSessions={showSessions}
          server={remote.server}
          shellOpen={shellOpen}
          state={status.state}
          threads={threads}
        />
        <View style={styles.main}>
          <View style={styles.header}>
            <View style={styles.headerText}>
              <Text style={styles.title}>
                {shellOpen ? "Shell" : (activeChat?.title || "Sessions")}
              </Text>
              <Text style={styles.subtitle}>
                {shellOpen
                  ? (remote.workerId ?? "No worker connected")
                  : activeChat
                    ? `${activeChat.messages.length} messages`
                    : "Goble desktop shell"}
              </Text>
            </View>
            <StatusPill label={status.label} state={status.state} />
          </View>

          {shellOpen ? (
            <TerminalPane />
          ) : activeChat ? (
            <ConversationPane
              chat={activeChat}
              focusSignal={composerFocusSignal}
              key={activeChat.id}
              live={remote.status === "live"}
            />
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

              <View style={styles.card}>
                <SectionHeader title={`Agents · ${entities.agents.length}`} />
                {entities.agents.length === 0 ? (
                  <Text style={styles.entityEmpty}>No agents stored.</Text>
                ) : (
                  <View style={styles.entities}>
                    {entities.agents.map((agent) => {
                      const updated = relativeTime(agent.updated_at);
                      return (
                        <View key={agent.id} style={styles.entityRow}>
                          <View style={styles.entityText}>
                            <Text numberOfLines={1} style={styles.entityName}>
                              {agent.name || agent.id}
                            </Text>
                            <Text numberOfLines={1} style={styles.entityMeta}>
                              {agent.id}
                              {updated ? ` · Updated ${updated}` : ""}
                            </Text>
                          </View>
                        </View>
                      );
                    })}
                  </View>
                )}
                {entities.agentsError ? (
                  <Text style={styles.entityError}>
                    {entities.agentsError}
                  </Text>
                ) : null}
              </View>

              <View style={styles.card}>
                <SectionHeader
                  title={`Scheduled · ${entities.schedules.length}`}
                />
                {entities.schedules.length === 0 ? (
                  <Text style={styles.entityEmpty}>No scheduled tasks.</Text>
                ) : (
                  <View style={styles.entities}>
                    {entities.schedules.map((task) => (
                      <View key={task.id} style={styles.entityRow}>
                        <Text numberOfLines={1} style={styles.entityAgent}>
                          {task.agent_id}
                        </Text>
                        <Text
                          style={[
                            styles.entityStatus,
                            task.enabled
                              ? styles.entityEnabled
                              : styles.entityDisabled,
                          ]}
                        >
                          {task.enabled ? "Enabled" : "Disabled"}
                        </Text>
                      </View>
                    ))}
                  </View>
                )}
                {entities.schedulesError ? (
                  <Text style={styles.entityError}>
                    {entities.schedulesError}
                  </Text>
                ) : null}
              </View>

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
  entities: {
    gap: 2,
  },
  entityRow: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    justifyContent: "space-between",
    paddingHorizontal: space.sm,
    paddingVertical: 7,
  },
  entityText: {
    flex: 1,
    gap: 1,
  },
  entityName: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
    fontWeight: "500",
  },
  entityMeta: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 11,
  },
  entityAgent: {
    ...typography.mono,
    color: palette.muted,
    flexShrink: 1,
    fontSize: 11,
  },
  entityStatus: {
    ...typography.caption,
    fontSize: 11,
  },
  entityEnabled: {
    color: palette.success,
    fontWeight: "600",
  },
  entityDisabled: {
    color: palette.subtle,
  },
  entityEmpty: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 12,
  },
  entityError: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
    paddingTop: space.sm,
  },
  pressed: {
    opacity: 0.65,
  },
});
