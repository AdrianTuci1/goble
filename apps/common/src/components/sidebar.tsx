import { Ionicons } from "@expo/vector-icons";
import { usePathname, useRouter } from "expo-router";
import { useEffect, useMemo, useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { getPlatformHaptics } from "../platform";
import { harnessMark } from "../lib/harness";
import { demoConversations, subscribeConversations } from "../lib/mock";
import { useConnection } from "../lib/remote/connection";
import { threadConversation, useWorkerThreads } from "../lib/threads";
import {
  palette,
  radius,
  sessionStatusLabel,
  space,
  typography,
} from "../lib/theme";
import type { Conversation } from "../lib/types";

import { Avatar } from "./avatar";
import { BrandMark } from "./brand-mark";
import { HarnessMark } from "./harness-mark";

export function Sidebar({ onClose }: { onClose: () => void }) {
  const router = useRouter();
  const pathname = usePathname();
  const remote = useConnection();
  const workerThreads = useWorkerThreads();
  const [localConversations, setLocalConversations] = useState<Conversation[]>(
    () => demoConversations(),
  );

  useEffect(
    () =>
      subscribeConversations(() => setLocalConversations(demoConversations())),
    [],
  );

  /**
   * Behind a gateway the drawer lists the worker's conversations, which are the
   * ones a session screen opens and the ones that survive the app closing.
   * Without one it lists the demo workspace, so the layout stays reviewable.
   */
  const conversations = useMemo(() => {
    if (remote.phase === "demo") return localConversations;
    return workerThreads.map(threadConversation);
  }, [remote.phase, workerThreads, localConversations]);

  const starred = conversations.filter((item) => item.starred);
  const recent = conversations.filter((item) => !item.starred);

  function go(path: string, replace = false) {
    getPlatformHaptics().tap();
    if (replace) router.replace(path);
    else router.push(path);
    onClose();
  }

  const agentsActive = pathname.startsWith("/agents");
  const scheduledActive = pathname.startsWith("/scheduled");
  const customizeActive = pathname.startsWith("/customize");

  return (
    <SafeAreaView
      edges={["top", "bottom", "left"]}
      style={styles.safe}
      testID="sessions-drawer"
    >
      <View style={styles.header}>
        <View style={styles.brandRow}>
          <BrandMark size={28} />
          <Text style={styles.brand}>Goble</Text>
        </View>
        <Pressable
          accessibilityLabel="Close sessions"
          accessibilityRole="button"
          hitSlop={8}
          onPress={onClose}
          style={({ pressed }) => [styles.close, pressed && styles.pressed]}
          testID="close-sessions"
        >
          <Ionicons name="close" size={18} color={palette.text} />
        </Pressable>
      </View>

      <ScrollView
        contentContainerStyle={styles.scroll}
        showsVerticalScrollIndicator={false}
      >
        <Pressable
          accessibilityRole="button"
          onPress={() => go("/", true)}
          style={({ pressed }) => [styles.action, pressed && styles.pressed]}
        >
          <View style={styles.actionIcon}>
            <Ionicons name="create-outline" size={18} color={palette.text} />
          </View>
          <Text style={styles.actionLabel}>New session</Text>
        </Pressable>

        <Pressable
          accessibilityRole="button"
          accessibilityState={{ selected: agentsActive }}
          onPress={() => go("/agents")}
          style={({ pressed }) => [
            styles.action,
            agentsActive && styles.actionActive,
            pressed && styles.pressed,
          ]}
        >
          <View style={styles.actionIcon}>
            <Ionicons name="people-outline" size={18} color={palette.muted} />
          </View>
          <Text style={styles.actionLabel}>Agents</Text>
        </Pressable>

        <Pressable
          accessibilityRole="button"
          accessibilityState={{ selected: scheduledActive }}
          onPress={() => go("/scheduled")}
          style={({ pressed }) => [
            styles.action,
            scheduledActive && styles.actionActive,
            pressed && styles.pressed,
          ]}
        >
          <View style={styles.actionIcon}>
            <Ionicons name="calendar-outline" size={18} color={palette.muted} />
          </View>
          <Text style={styles.actionLabel}>Scheduled</Text>
        </Pressable>

        <Pressable
          accessibilityRole="button"
          accessibilityState={{ selected: customizeActive }}
          onPress={() => go("/customize")}
          style={({ pressed }) => [
            styles.action,
            customizeActive && styles.actionActive,
            pressed && styles.pressed,
          ]}
        >
          <View style={styles.actionIcon}>
            <Ionicons
              name="color-palette-outline"
              size={18}
              color={palette.muted}
            />
          </View>
          <Text style={styles.actionLabel}>Customize</Text>
        </Pressable>

        {conversations.length === 0 ? (
          <Text style={styles.emptyList}>
            No conversations on this gateway yet. Ask something in the composer
            and it will appear here.
          </Text>
        ) : null}

        {starred.length > 0 ? (
          <>
            <Text style={styles.sectionLabel}>
              Starred · {starred.length}
            </Text>
            <View style={styles.rows}>
              {starred.map((conversation) => (
                <ConversationRow
                  conversation={conversation}
                  key={conversation.id}
                  onPress={() => go(`/session/${conversation.id}`)}
                  starred
                  active={pathname === `/session/${conversation.id}`}
                />
              ))}
            </View>
          </>
        ) : null}

        {recent.length > 0 ? (
          <>
            <Text style={styles.sectionLabel}>
              Recent · {recent.length}
            </Text>
            <View style={styles.rows}>
              {recent.map((conversation) => (
                <ConversationRow
                  conversation={conversation}
                  key={conversation.id}
                  onPress={() => go(`/session/${conversation.id}`)}
                  active={pathname === `/session/${conversation.id}`}
                />
              ))}
            </View>
          </>
        ) : null}
      </ScrollView>

      <View style={styles.footer}>
        <View style={styles.account}>
          <Avatar size={34} />
          <View style={styles.accountText}>
            <Text style={styles.accountName}>Your workspace</Text>
            <Text style={styles.accountPlan}>Personal</Text>
          </View>
          <Pressable
            accessibilityLabel="Settings"
            accessibilityRole="button"
            hitSlop={8}
            onPress={() => go("/settings")}
            style={({ pressed }) => [styles.gear, pressed && styles.pressed]}
            testID="sidebar-settings"
          >
            <Ionicons
              name="settings-outline"
              size={19}
              color={palette.muted}
            />
          </Pressable>
        </View>
      </View>
    </SafeAreaView>
  );
}

function ConversationRow({
  conversation,
  active,
  starred = false,
  onPress,
}: {
  conversation: Conversation;
  active: boolean;
  starred?: boolean;
  onPress: () => void;
}) {
  const harness = harnessMark(conversation.harness);

  return (
    <Pressable
      accessibilityLabel={`${conversation.title}, ${harness.label}, ${sessionStatusLabel[conversation.status]}`}
      accessibilityRole="button"
      accessibilityState={{ selected: active }}
      onPress={onPress}
      style={({ pressed }) => [
        styles.row,
        active && styles.rowActive,
        pressed && styles.pressed,
      ]}
    >
      <HarnessMark
        harness={conversation.harness}
        ringColor={active ? palette.raised : palette.bg}
        status={conversation.status}
      />
      <View style={styles.rowText}>
        <Text
          numberOfLines={1}
          style={[styles.rowTitle, active && styles.rowTitleActive]}
        >
          {conversation.title}
        </Text>
        <Text numberOfLines={1} style={styles.rowDetail}>
          {conversation.preview}
        </Text>
      </View>
      {starred ? (
        <Ionicons name="star" size={12} color={palette.warning} />
      ) : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  safe: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  header: {
    alignItems: "center",
    flexDirection: "row",
    justifyContent: "space-between",
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
  },
  brandRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
  },
  brand: {
    ...typography.title,
    color: palette.text,
    fontSize: 22,
    letterSpacing: -0.5,
  },
  close: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    height: 36,
    justifyContent: "center",
    width: 36,
  },
  scroll: {
    paddingBottom: space.lg,
    paddingHorizontal: space.lg,
  },
  action: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.sm,
    paddingVertical: 10,
  },
  actionActive: {
    backgroundColor: palette.raised,
  },
  actionIcon: {
    alignItems: "center",
    height: 32,
    justifyContent: "center",
    width: 32,
  },
  actionLabel: {
    ...typography.callout,
    color: palette.text,
    fontSize: 15,
  },
  emptyList: {
    ...typography.caption,
    color: palette.subtle,
    lineHeight: 18,
    paddingHorizontal: space.sm,
    paddingTop: space.xl,
  },
  sectionLabel: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
    fontWeight: "700",
    letterSpacing: 1,
    marginTop: space.xl,
    paddingHorizontal: space.sm,
    paddingVertical: space.sm,
    textTransform: "uppercase",
  },
  rows: {
    gap: 2,
  },
  row: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.sm,
    paddingVertical: 9,
  },
  rowActive: {
    backgroundColor: palette.raised,
  },
  rowText: {
    flex: 1,
    gap: 2,
  },
  rowTitle: {
    ...typography.body,
    color: palette.muted,
    fontSize: 14,
  },
  rowTitleActive: {
    color: palette.text,
    fontWeight: "600",
  },
  rowDetail: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
  },
  footer: {
    borderTopColor: palette.border,
    borderTopWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: space.lg,
    paddingTop: space.sm,
  },
  account: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    paddingVertical: space.sm,
  },
  accountText: {
    flex: 1,
    gap: 1,
  },
  accountName: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
  },
  accountPlan: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 11,
  },
  gear: {
    alignItems: "center",
    height: 34,
    justifyContent: "center",
    width: 34,
  },
  pressed: {
    opacity: 0.65,
  },
});
