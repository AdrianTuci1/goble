import { Ionicons } from "@expo/vector-icons";
import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";

import {
  RichText,
  palette,
  radius,
  requestChat,
  space,
  typography,
  type RemoteChat,
} from "@goble/common";

/** One transcript read: loading, the stored chat, or the reason it is not drawn. */
type Load =
  | { kind: "loading" }
  | { kind: "ready"; chat: RemoteChat }
  | { kind: "missing"; reason: string | null };

function describe(error: unknown): string {
  return error instanceof Error ? error.message : "The worker did not answer";
}

/** Name the stored role without rewriting the word the worker used. */
function roleLabel(role: string): string {
  if (role === "user") return "You";
  if (role === "assistant") return "Agent";
  return role ? role.charAt(0).toUpperCase() + role.slice(1) : "Message";
}

/**
 * A subagent's stored transcript, drawn read-only in the main pane.
 *
 * A subagent is its own stored conversation, so the worker holds the messages;
 * this reads them with `requestChat` and never mounts a composer, because the
 * child's input belongs to the parent. The back bar returns to the parent
 * conversation and stays available while the read is loading or has failed.
 */
export function SubagentPane({
  subagentId,
  title,
  parentTitle,
  live,
  onBack,
}: {
  subagentId: string;
  title: string;
  parentTitle: string | null;
  live: boolean;
  onBack: () => void;
}) {
  const [load, setLoad] = useState<Load>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;

    if (!live) {
      setLoad({
        kind: "missing",
        reason: "The worker link is not live.",
      });
      return () => {
        cancelled = true;
      };
    }

    setLoad({ kind: "loading" });
    void requestChat(subagentId)
      .then((chat) => {
        if (cancelled) return;
        setLoad(
          chat
            ? { kind: "ready", chat }
            : { kind: "missing", reason: null },
        );
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setLoad({ kind: "missing", reason: describe(error) });
      });

    return () => {
      cancelled = true;
    };
  }, [live, subagentId]);

  const messages =
    load.kind === "ready"
      ? load.chat.messages.filter((message) => message.content.trim().length > 0)
      : [];

  return (
    <View style={styles.root}>
      <View style={styles.backBar}>
        <Pressable
          accessibilityLabel="Back to parent conversation"
          accessibilityRole="button"
          onPress={onBack}
          style={({ pressed }) => [styles.back, pressed && styles.pressed]}
        >
          <Ionicons
            aria-hidden
            color={palette.muted}
            name="arrow-back"
            size={15}
          />
          <Text style={styles.backLabel}>Back</Text>
        </Pressable>
        <View style={styles.barText}>
          <Text numberOfLines={1} style={styles.barTitle}>
            Subagent · {title || "Untitled subagent"}
          </Text>
          <Text numberOfLines={1} style={styles.barMeta}>
            {parentTitle ? `Parent · ${parentTitle}` : "Parent conversation not stored"}
          </Text>
        </View>
      </View>

      {load.kind === "loading" ? (
        <View style={styles.state}>
          <ActivityIndicator color={palette.accent} size="small" />
          <Text style={styles.stateBody}>Loading transcript…</Text>
        </View>
      ) : load.kind === "ready" ? (
        <ScrollView
          contentContainerStyle={styles.transcript}
          showsVerticalScrollIndicator={false}
        >
          {messages.length === 0 ? (
            <Text style={styles.empty}>
              This subagent has no stored messages.
            </Text>
          ) : (
            messages.map((message) => (
              <View key={message.id} style={styles.row}>
                <View
                  style={[
                    styles.bubble,
                    message.role === "user"
                      ? styles.userBubble
                      : styles.agentBubble,
                  ]}
                >
                  <Text style={styles.author}>{roleLabel(message.role)}</Text>
                  <RichText style={styles.body} text={message.content} />
                </View>
              </View>
            ))
          )}
        </ScrollView>
      ) : (
        <View style={styles.state}>
          <Ionicons
            aria-hidden
            color={palette.muted}
            name="alert-circle-outline"
            size={22}
          />
          <Text style={styles.stateTitle}>Transcript unavailable</Text>
          <Text style={styles.stateBody}>
            This subagent's transcript is no longer on the worker.
          </Text>
          {load.reason ? (
            <Text style={styles.stateReason}>{load.reason}</Text>
          ) : null}
        </View>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
  backBar: {
    alignItems: "center",
    borderBottomColor: palette.border,
    borderBottomWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.xl,
    paddingVertical: space.md,
  },
  back: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderRadius: radius.sm,
    flexDirection: "row",
    gap: 4,
    paddingHorizontal: space.sm,
    paddingVertical: 5,
  },
  backLabel: {
    ...typography.caption,
    color: palette.text,
    fontSize: 12,
    fontWeight: "500",
  },
  barText: {
    flex: 1,
    gap: 1,
  },
  barTitle: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
    fontWeight: "500",
  },
  barMeta: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
  },
  transcript: {
    alignSelf: "center",
    gap: space.md,
    maxWidth: 860,
    padding: space.xl,
    width: "100%",
  },
  empty: {
    ...typography.caption,
    color: palette.muted,
    paddingVertical: space.xxl,
    textAlign: "center",
  },
  // Both roles share one full-width column: the row stretches its bubble to the
  // same 860px content width and the text always starts at the same left edge.
  // The role is told by the fill/border and the author label, never by width.
  row: {
    alignItems: "stretch",
  },
  bubble: {
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    gap: 4,
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
  },
  agentBubble: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
  },
  userBubble: {
    backgroundColor: palette.accentSoft,
    borderColor: palette.accent,
  },
  author: {
    ...typography.eyebrow,
    color: palette.subtle,
    fontSize: 10,
    textAlign: "left",
  },
  body: {
    ...typography.body,
    color: palette.text,
    fontSize: 14,
    lineHeight: 20,
    textAlign: "left",
  },
  state: {
    alignItems: "center",
    flex: 1,
    gap: space.sm,
    justifyContent: "center",
    padding: space.xl,
  },
  stateTitle: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  stateBody: {
    ...typography.caption,
    color: palette.muted,
    lineHeight: 18,
    textAlign: "center",
  },
  stateReason: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
    textAlign: "center",
  },
  pressed: {
    opacity: 0.7,
  },
});
