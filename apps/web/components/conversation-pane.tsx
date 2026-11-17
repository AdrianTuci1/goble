import { Ionicons } from "@expo/vector-icons";
import { useEffect, useRef, useState } from "react";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
  type ScrollViewInstance,
  type TextInputInstance,
} from "react-native";

import {
  RichText,
  palette,
  radius,
  refreshThreads,
  space,
  startTurn,
  typography,
  type RemoteChat,
  type Turn,
} from "@goble/common";

/** One line the pane draws: a stored message or one this turn produced. */
interface Line {
  id: string;
  role: "user" | "assistant";
  text: string;
}

type Pending =
  | { kind: "question"; text: string; replies: string[] }
  | { kind: "command"; candidates: string[]; cwd: string }
  | null;

function stored(chat: RemoteChat): Line[] {
  return chat.messages
    .filter(
      (message) =>
        (message.role === "user" || message.role === "assistant") &&
        message.content.trim().length > 0,
    )
    .map((message) => ({
      id: message.id,
      role: message.role as "user" | "assistant",
      text: message.content,
    }));
}

/**
 * One conversation: its stored transcript, plus a turn this pane started.
 *
 * The pane sends `run_agent_for_thread_reply`, which the worker keys on the
 * conversation id, so the reply lands in the same transcript the phone reads.
 * Deltas are drawn as they arrive; a question or a proposed command pauses the
 * run and is answered here rather than from the phone. A turn this shell cannot
 * finish — a command proposal it rejects — is visible instead of silently
 * hanging: the decision buttons always leave the run in a known state.
 */
export function ConversationPane({
  chat,
  live,
  focusSignal,
}: {
  chat: RemoteChat;
  live: boolean;
  focusSignal: number;
}) {
  const [local, setLocal] = useState<Line[] | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Pending>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const replyId = useRef(`web-reply-${chat.id}`);
  const turn = useRef<Turn | null>(null);
  const transcript = useRef<ScrollViewInstance>(null);
  const input = useRef<TextInputInstance>(null);
  const focusSeen = useRef(focusSignal);

  const lines = local ?? stored(chat);

  useEffect(() => {
    return () => turn.current?.detach();
  }, []);

  useEffect(() => {
    if (focusSignal === focusSeen.current) return;
    focusSeen.current = focusSignal;
    input.current?.focus();
  }, [focusSignal]);

  useEffect(() => {
    const timer = setTimeout(
      () => transcript.current?.scrollToEnd({ animated: false }),
      40,
    );
    return () => clearTimeout(timer);
  }, [lines]);

  function add(line: Line) {
    setLocal((current) => [...(current ?? stored(chat)), line]);
  }

  function appendToReply(delta: string) {
    setNotice(null);
    setLocal((current) => {
      if (!current) return current;
      const index = current.findIndex((line) => line.id === replyId.current);
      if (index === -1) {
        return [...current, { id: replyId.current, role: "assistant", text: delta }];
      }
      return current.map((line) =>
        line.id === replyId.current
          ? { ...line, text: line.text + delta }
          : line,
      );
    });
  }

  function finish() {
    setBusy(false);
    setPending(null);
    setNotice(null);
    turn.current = null;
    refreshThreads();
  }

  function send() {
    const prompt = draft.trim();
    if (!prompt) return;

    if (pending?.kind === "question") {
      answer(prompt);
      return;
    }
    if (busy || !live || pending?.kind === "command") return;

    setDraft("");
    setNotice("Waiting for the agent…");
    replyId.current = `web-reply-${Date.now()}`;
    setLocal([
      ...lines,
      { id: `web-user-${Date.now()}`, role: "user", text: prompt },
    ]);
    setBusy(true);

    const started = startTurn(
      chat.id,
      prompt,
      {
        onDelta: appendToReply,
        onToolStarted: (call) => setNotice(`Running ${call.name}…`),
        onToolFinished: () => setNotice(null),
        onAskUser: (question, replies) => {
          add({ id: `web-question-${Date.now()}`, role: "assistant", text: question });
          setPending({ kind: "question", text: question, replies });
          setBusy(false);
          setNotice(null);
        },
        onCommandProposed: (proposal) => {
          setPending({
            kind: "command",
            candidates: proposal.candidates,
            cwd: proposal.cwd,
          });
          setBusy(false);
          setNotice(null);
        },
        onDone: finish,
        onError: (message) => {
          add({
            id: `web-error-${Date.now()}`,
            role: "assistant",
            text: message,
          });
          finish();
        },
      },
      chat.title,
    );

    if (!started) {
      setLocal(null);
      setBusy(false);
      setNotice("The worker link is not live, so the message was not sent.");
      return;
    }
    turn.current = started;
  }

  function answer(text: string) {
    const current = turn.current;
    setDraft("");
    setPending(null);
    setNotice(null);
    if (!current || !current.answer(text)) {
      setNotice("This run is no longer attached to the page.");
      return;
    }
    replyId.current = `web-reply-${Date.now()}`;
    add({ id: `web-user-${Date.now()}`, role: "user", text });
    setBusy(true);
    setNotice("Waiting for the agent…");
  }

  function decide(type: "approve" | "reject") {
    if (pending?.kind !== "command") return;
    const current = turn.current;
    const candidate = pending.candidates[0] ?? "";
    setPending(null);
    setNotice(null);
    if (!current?.decideCommand({ type, payload: candidate })) {
      setNotice("This run is no longer attached to the page.");
      return;
    }
    add({
      id: `web-command-${Date.now()}`,
      role: "assistant",
      text:
        type === "approve"
          ? `Running \`${candidate}\``
          : "Command rejected.",
    });
    setBusy(true);
  }

  const canSend = draft.trim().length > 0 && !busy;

  return (
    <View style={styles.root}>
      <ScrollView
        contentContainerStyle={styles.transcript}
        ref={transcript}
        showsVerticalScrollIndicator={false}
      >
        {lines.length === 0 ? (
          <Text style={styles.empty}>
            No messages in this conversation yet.
          </Text>
        ) : (
          lines.map((line) => (
            <View key={line.id} style={styles.row}>
              <View
                style={[
                  styles.bubble,
                  line.role === "user" ? styles.userBubble : styles.agentBubble,
                ]}
              >
                <Text style={styles.author}>
                  {line.role === "user" ? "You" : "Agent"}
                </Text>
                {line.text ? (
                  <RichText style={styles.body} text={line.text} />
                ) : null}
              </View>
            </View>
          ))
        )}
      </ScrollView>

      {pending?.kind === "question" ? (
        <View style={styles.pending}>
          {pending.replies.map((reply) => (
            <Pressable
              accessibilityRole="button"
              key={reply}
              onPress={() => answer(reply)}
              style={({ pressed }) => [
                styles.quickReply,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.quickReplyLabel}>{reply}</Text>
            </Pressable>
          ))}
        </View>
      ) : null}

      {pending?.kind === "command" ? (
        <View style={styles.proposal}>
          <Text style={styles.proposalHead}>
            The agent wants to run this in {pending.cwd || "the workspace"}:
          </Text>
          {pending.candidates.map((candidate) => (
            <Text key={candidate} style={styles.proposalLine}>
              {candidate}
            </Text>
          ))}
          <View style={styles.proposalActions}>
            <Pressable
              accessibilityRole="button"
              onPress={() => decide("approve")}
              style={({ pressed }) => [
                styles.approve,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.approveLabel}>Run it</Text>
            </Pressable>
            <Pressable
              accessibilityRole="button"
              onPress={() => decide("reject")}
              style={({ pressed }) => [
                styles.reject,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.rejectLabel}>Reject</Text>
            </Pressable>
          </View>
        </View>
      ) : null}

      {notice ? <Text style={styles.notice}>{notice}</Text> : null}

      <View style={styles.composer}>
        <TextInput
          accessibilityLabel="Message the agent"
          autoCapitalize="sentences"
          autoCorrect
          editable={live && pending?.kind !== "command"}
          onChangeText={setDraft}
          onSubmitEditing={send}
          placeholder={
            live
              ? pending?.kind === "question"
                ? "Answer the agent"
                : "Message the agent"
              : "The worker link is not live"
          }
          placeholderTextColor={palette.subtle}
          ref={input}
          returnKeyType="send"
          selectionColor={palette.accent}
          style={styles.input}
          value={draft}
        />
        <Pressable
          accessibilityLabel="Send message"
          accessibilityRole="button"
          disabled={!canSend || !live}
          onPress={send}
          style={({ pressed }) => [
            styles.send,
            (!canSend || !live) && styles.sendDisabled,
            pressed && styles.pressed,
          ]}
        >
          <Ionicons
            aria-hidden
            color={canSend && live ? palette.bg : palette.subtle}
            name="arrow-up"
            size={16}
          />
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
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
  pending: {
    alignSelf: "center",
    flexDirection: "row",
    flexWrap: "wrap",
    gap: space.sm,
    maxWidth: 860,
    paddingHorizontal: space.xl,
    width: "100%",
  },
  quickReply: {
    backgroundColor: palette.raised,
    borderColor: palette.borderStrong,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: space.md,
    paddingVertical: 6,
  },
  quickReplyLabel: {
    ...typography.caption,
    color: palette.text,
    fontSize: 12,
  },
  proposal: {
    alignSelf: "center",
    backgroundColor: palette.surface,
    borderColor: palette.warning,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    gap: 6,
    marginHorizontal: space.xl,
    marginTop: space.sm,
    maxWidth: 860,
    padding: space.md,
  },
  proposalHead: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
  },
  proposalLine: {
    ...typography.mono,
    color: palette.command,
    fontSize: 12,
    lineHeight: 17,
  },
  proposalActions: {
    flexDirection: "row",
    gap: space.sm,
    marginTop: 2,
  },
  approve: {
    backgroundColor: palette.warning,
    borderRadius: radius.sm,
    paddingHorizontal: space.md,
    paddingVertical: 6,
  },
  approveLabel: {
    ...typography.caption,
    color: palette.bg,
    fontSize: 12,
    fontWeight: "700",
  },
  reject: {
    backgroundColor: palette.raised,
    borderRadius: radius.sm,
    paddingHorizontal: space.md,
    paddingVertical: 6,
  },
  rejectLabel: {
    ...typography.caption,
    color: palette.text,
    fontSize: 12,
  },
  notice: {
    ...typography.caption,
    alignSelf: "center",
    color: palette.muted,
    fontSize: 11,
    paddingHorizontal: space.xl,
    paddingTop: space.sm,
  },
  composer: {
    alignItems: "center",
    alignSelf: "center",
    flexDirection: "row",
    gap: space.sm,
    maxWidth: 860,
    paddingHorizontal: space.xl,
    paddingVertical: space.lg,
    width: "100%",
  },
  input: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    color: palette.text,
    flex: 1,
    fontSize: 14,
    paddingHorizontal: space.md,
    paddingVertical: 10,
  },
  send: {
    alignItems: "center",
    backgroundColor: palette.accent,
    borderRadius: radius.md,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  sendDisabled: {
    backgroundColor: palette.raised,
  },
  pressed: {
    opacity: 0.7,
  },
});
