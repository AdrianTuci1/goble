import { useMemo, useState } from "react";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import {
  EmptyState,
  SectionHeader,
  StatusPill,
  connection,
  demoConversation,
  demoConversations,
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
  type RemoteChat,
  type RemoteSubagent,
  type SessionStatus,
} from "@goble/common";

import { ChatSidebar } from "../components/chat-sidebar";
import { ConversationPane } from "../components/conversation-pane";
import {
  Sidebar,
  type SidebarGroup,
  type ThreadRow,
} from "../components/sidebar";
import { SubagentPane } from "../components/subagent-pane";
import { TerminalPane } from "../components/terminal-pane";
import {
  RunsCard,
  SubagentsCard,
  WorkflowsCard,
} from "../components/work-cards";
import { useShellShortcuts } from "../lib/shortcuts";
import { useWorkerEntities } from "../lib/worker-entities";
import { useWorkerWork } from "../lib/worker-work";

/**
 * Width at which the third column is composed. The left and right columns take
 * 280 each, so the centre keeps 540 px here: enough for the transcript bubbles
 * and the composer without squeezing either.
 */
const WIDE_LAYOUT_MIN_WIDTH = 1100;

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
  const { width } = useWindowDimensions();
  const status = badge(remote.phase, remote.status);
  const chats = useWorkerThreads();
  const entities = useWorkerEntities();
  const work = useWorkerWork();
  const [shellOpen, setShellOpen] = useState(false);
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [openSubagent, setOpenSubagent] = useState<RemoteSubagent | null>(null);
  const [composerFocusSignal, setComposerFocusSignal] = useState(0);

  /**
   * A sample workspace, opt-in only, for design review of the populated
   * sidebar (`EXPO_PUBLIC_GOBLE_DEMO=1`). It must never stand in for a real
   * worker: an unpaired shell keeps showing the "No worker connected" empty
   * state. The demo turns stay read-only because the relay is not live.
   */
  const demoWorkspaceEnabled =
    process.env.EXPO_PUBLIC_GOBLE_DEMO === "1";
  const demo = remote.phase === "demo" && demoWorkspaceEnabled;
  const demoConversationsList = useMemo(
    () => (demo ? demoConversations() : []),
    [demo],
  );
  const demoChats = useMemo(
    () =>
      demoConversationsList.map((conversation) => {
        const detail = demoConversation(conversation.id);
        return {
          created_at: conversation.updatedAt,
          id: conversation.id,
          messages: detail.messages
            .filter(
              (message) =>
                message.role === "user" || message.role === "assistant",
            )
            .map((message) => ({
              content: message.text,
              created_at: message.at,
              id: message.id,
              role: message.role,
            })),
          parent_chat_id: null,
          title: conversation.title,
          updated_at: conversation.updatedAt,
        } satisfies RemoteChat;
      }),
    [demoConversationsList],
  );
  const sourceChats = demo ? demoChats : chats;
  /** A shell is one worker's child; with none selected there is nothing to open. */
  const canShell = remote.workerId !== null;
  const activeChat =
    sourceChats.find((thread) => thread.id === activeThreadId) ?? null;
  /** A read-only subagent transcript is not a conversation for the composer. */
  const conversationOpen =
    !shellOpen && openSubagent === null && activeChat !== null;
  const showChatSidebar = width >= WIDE_LAYOUT_MIN_WIDTH && conversationOpen;
  const conversationSubagents = activeChat
    ? work.subagents.filter(
        (subagent) => subagent.parent_chat_id === activeChat.id,
      )
    : [];

  function showSessions() {
    setShellOpen(false);
    setOpenSubagent(null);
    setActiveThreadId(null);
  }

  function openShell() {
    setOpenSubagent(null);
    setShellOpen(true);
  }

  function openSubagentTranscript(subagent: RemoteSubagent) {
    setShellOpen(false);
    setActiveThreadId(subagent.parent_chat_id);
    setOpenSubagent(subagent);
  }

  function closeSubagent() {
    const parentId = openSubagent?.parent_chat_id ?? null;
    setOpenSubagent(null);
    setShellOpen(false);
    setActiveThreadId(parentId);
  }

  useShellShortcuts({
    canShell,
    shellOpen,
    conversationOpen,
    subagentOpen: openSubagent !== null,
    onClearConversation: () => setActiveThreadId(null),
    onCloseSubagent: closeSubagent,
    onFocusComposer: () => setComposerFocusSignal((value) => value + 1),
    onNewShell: openShell,
    onOpenShell: openShell,
    onShowSessions: showSessions,
  });

  /**
   * The sidebar's groups are the conversations' project names — the DSH
   * "Workspaces" grouping. Worker conversations all carry "Gateway" today
   * (`threadConversation`), so production renders one group; the demo
   * workspace carries three real project names.
   */
  const groups: SidebarGroup[] = useMemo(() => {
    const byProject = new Map<string, ThreadRow[]>();
    if (demo) {
      for (const conversation of demoConversationsList) {
        const row: ThreadRow = {
          createdAt: conversation.updatedAt,
          id: conversation.id,
          preview: conversation.preview,
          status: conversation.status,
          title: conversation.title,
          updatedAt: conversation.updatedAt,
        };
        const project = conversation.projectName || "General";
        const rows = byProject.get(project);
        if (rows === undefined) byProject.set(project, [row]);
        else rows.push(row);
      }
    } else {
      for (const chat of chats) {
        const thread = threadConversation(chat);
        // A stored conversation whose last message is still the user's is
        // waiting for the worker's turn: DSH draws that as the running ring.
        // Everything else is idle and carries no leading dot.
        const last = chat.messages[chat.messages.length - 1];
        const status: SessionStatus = last?.role === "user" ? "live" : "done";
        const row: ThreadRow = {
          createdAt: chat.created_at,
          id: thread.id,
          messageCount: chat.messages.length,
          preview: thread.preview,
          status,
          title: thread.title,
          updatedAt: chat.updated_at,
        };
        const project = thread.projectName || "General";
        const rows = byProject.get(project);
        if (rows === undefined) byProject.set(project, [row]);
        else rows.push(row);
      }
    }
    return [...byProject].map(([label, threads]) => ({
      id: `project:${label}`,
      label,
      threads,
    }));
  }, [chats, demo, demoConversationsList]);

  return (
    <SafeAreaView
      edges={["top", "bottom", "left", "right"]}
      style={styles.safe}
    >
      <View style={styles.shell}>
        <Sidebar
          activeThreadId={activeThreadId}
          /* The design-review demo workspace renders the create affordances in
             their connected state; a normal unpaired shell keeps them disabled. */
          canShell={canShell || demoWorkspaceEnabled}
          groups={groups}
          label={status.label}
          onNewShell={openShell}
          onOpenShell={openShell}
          onSelectThread={(id) => {
            setShellOpen(false);
            setOpenSubagent(null);
            setActiveThreadId(id);
          }}
          onShowSessions={showSessions}
          server={remote.server}
          shellOpen={shellOpen}
          state={status.state}
        />
        <View style={styles.main}>
          <View style={styles.header}>
            <View style={styles.headerText}>
              <Text style={styles.title}>
                {shellOpen
                  ? "Shell"
                  : openSubagent
                    ? (openSubagent.title || "Subagent")
                    : activeChat?.title || "Sessions"}
              </Text>
              <Text style={styles.subtitle}>
                {shellOpen
                  ? (remote.workerId ?? "No worker connected")
                  : openSubagent
                    ? openSubagent.parent_title
                      ? `Subagent of ${openSubagent.parent_title}`
                      : "Parent conversation not stored"
                    : activeChat
                      ? `${activeChat.messages.length} messages`
                      : "Goble desktop shell"}
              </Text>
            </View>
            <StatusPill label={status.label} state={status.state} />
          </View>

          {shellOpen ? (
            <TerminalPane />
          ) : openSubagent ? (
            <SubagentPane
              key={openSubagent.id}
              live={remote.status === "live"}
              onBack={closeSubagent}
              parentTitle={openSubagent.parent_title}
              subagentId={openSubagent.id}
              title={openSubagent.title}
            />
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

              <WorkflowsCard
                error={work.workflowsError}
                workflows={work.workflows}
              />

              <RunsCard
                error={work.executionsError}
                executions={work.executions}
              />

              <SubagentsCard
                error={work.subagentsError}
                onOpenSubagent={openSubagentTranscript}
                subagents={work.subagents}
              />

              <EmptyState
                body={emptyBody(remote)}
                icon="chatbubble-ellipses-outline"
                title="No session selected"
              />
            </ScrollView>
          )}
        </View>
        {showChatSidebar ? (
          <ChatSidebar
            error={entities.schedulesError}
            onOpenSubagent={openSubagentTranscript}
            schedules={entities.schedules}
            subagents={conversationSubagents}
            subagentsError={work.subagentsError}
          />
        ) : null}
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
