import { Ionicons } from "@expo/vector-icons";
import { useLocalSearchParams, useRouter } from "expo-router";
import { useEffect, useRef, useState } from "react";
import {
  Image,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context";

import { Composer } from "../../../components/composer";
import { ComposerSheet } from "../../../components/composer-sheet";
import { ConversationMenu } from "../../../components/conversation-menu";
import { ModelMenu } from "../../../components/model-menu";
import { RichText } from "../../../components/rich-text";
import { TerminalBlock } from "../../../components/terminal-block";
import { TopBar } from "../../../components/top-bar";
import { CallCard } from "../../../components/call-card";
import {
  pickAttachment,
  type AttachmentSource,
} from "../../../lib/attachments";
import { tapFeedback } from "../../../lib/haptics";
import { useOpenDrawer } from "../../../lib/navigation";
import {
  demoConversation,
  rememberConversation,
  removeConversation,
  renameConversation,
  setConversationPreview,
  setConversationStarred,
  storeConversationMessages,
  subscribeConversations,
} from "../../../lib/mock";
import { palette, radius, space, typography } from "../../../lib/theme";
import type { ChatMessage, SessionMode, ThinkingLevel } from "../../../lib/types";
import {
  notifyAgentNeedsHelp,
  notifyRunFinished,
} from "../../../lib/notifications";
import { useModelChoices } from "../../../lib/use-models";
import type { ScrollViewInstance } from "react-native";

export default function SessionScreen() {
  const params = useLocalSearchParams<{ id: string }>();
  return <SessionView key={params.id ?? "new"} />;
}

function SessionView() {
  const router = useRouter();
  const insets = useSafeAreaInsets();
  const openDrawer = useOpenDrawer();
  const params = useLocalSearchParams<{
    id: string;
    prompt?: string;
    mode?: string;
    image?: string;
  }>();
  const id = params.id ?? "new";
  const initialMode: SessionMode = params.mode === "terminal" ? "terminal" : "agent";
  const detail = demoConversation(id, params.prompt, initialMode, params.image);
  const { defaultModel, models } = useModelChoices();

  const [model, setModel] = useState(defaultModel);
  const [thinking, setThinking] = useState<ThinkingLevel>("standard");
  const [mode, setMode] = useState<SessionMode>(detail.conversation.mode);
  const [conversation, setConversation] = useState(detail.conversation);
  const [messages, setMessages] = useState<ChatMessage[]>(detail.messages);
  const [prompt, setPrompt] = useState("");
  const [attachment, setAttachment] = useState<string | null>(null);
  const [attachSheet, setAttachSheet] = useState(false);
  const [modelMenu, setModelMenu] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [voiceOpen, setVoiceOpen] = useState(false);
  const scrollRef = useRef<ScrollViewInstance>(null);

  useEffect(() => {
    setModel(defaultModel);
  }, [defaultModel.id]);

  const needsHelp = messages.some(
    (message) =>
      message.role === "tool" && message.status === "waiting_approval",
  );
  const notifiedHelp = useRef<string | null>(null);

  useEffect(() => {
    if (!needsHelp) {
      notifiedHelp.current = null;
      return;
    }
    if (notifiedHelp.current === id) return;
    notifiedHelp.current = id;
    void notifyAgentNeedsHelp(conversation.title);
  }, [needsHelp, id, conversation.title]);

  useEffect(() => {
    rememberConversation(detail.conversation, detail.messages);
    return subscribeConversations(() => {
      setConversation(demoConversation(id).conversation);
    });
    // The screen is keyed by id, so the initial detail is the one to register.
  }, [id]);

  useEffect(() => {
    const timer = setTimeout(
      () => scrollRef.current?.scrollToEnd({ animated: true }),
      60,
    );
    return () => clearTimeout(timer);
  }, [messages]);

  function send(text = prompt) {
    const value = text.trim();
    if (!value && !attachment) return;
    const now = new Date().toISOString();
    const image = attachment ?? undefined;
    const userMessage: ChatMessage = {
      id: `user-${Date.now()}`,
      role: mode === "terminal" ? "terminal" : "user",
      text: value || "Attached image",
      command: mode === "terminal" ? value || "open image" : undefined,
      cwd: mode === "terminal" ? "workspace/goble" : undefined,
      image,
      at: now,
      status: mode === "terminal" ? "done" : undefined,
    };
    const reply: ChatMessage = {
      id: `reply-${Date.now()}`,
      role: "assistant",
      text:
        mode === "terminal"
          ? "The command is queued in this demo session. The gateway terminal bridge is the next slice."
          : "I have the prompt. The gateway run API is the next slice; until it lands this session stays local and shows the shape of the transcript.",
      at: now,
    };
    const next = [...messages, userMessage, reply];
    setMessages(next);
    storeConversationMessages(id, next);
    if (messages.length === 0) {
      renameConversation(id, (value || "Attached image").slice(0, 54));
    }
    setConversationPreview(id, value || "Attached image");
    setPrompt("");
    setAttachment(null);
    void notifyRunFinished(conversation.title);
  }

  function addVoiceExchange(exchange: { heard: string; reply: string }) {
    const now = new Date().toISOString();
    const next: ChatMessage[] = [
      ...messages,
      {
        id: `voice-user-${Date.now()}`,
        role: "user",
        text: exchange.heard,
        at: now,
      },
      {
        id: `voice-reply-${Date.now()}`,
        role: "assistant",
        text: exchange.reply,
        at: now,
      },
    ];
    setMessages(next);
    storeConversationMessages(id, next);
    if (messages.length === 0) {
      renameConversation(id, exchange.heard.slice(0, 54));
    }
    setConversationPreview(id, exchange.heard);
  }

  async function attach(source: AttachmentSource) {
    setAttachSheet(false);
    const uri = await pickAttachment(source);
    if (uri) setAttachment(uri);
  }

  return (
    <View style={styles.root}>
      <SafeAreaView style={styles.safe} edges={["top", "left", "right"]}>
      <TopBar
        model={model}
        onAvatar={() => router.push("/settings")}
        onMenu={openDrawer}
        onModel={() => setModelMenu(true)}
        right={
          <View style={styles.topActions}>
            <Pressable
              accessibilityLabel="New session"
              accessibilityRole="button"
              hitSlop={8}
              onPress={() => router.replace("/")}
              style={({ pressed }) => [
                styles.newButton,
                pressed && styles.pressed,
              ]}
            >
              <Ionicons name="create-outline" size={18} color={palette.text} />
            </Pressable>
            <Pressable
              accessibilityLabel="Session actions"
              accessibilityRole="button"
              hitSlop={8}
              onPress={() => {
                tapFeedback();
                setActionsOpen(true);
              }}
              style={({ pressed }) => [
                styles.newButton,
                pressed && styles.pressed,
              ]}
              testID="session-actions"
            >
              <Ionicons
                name="ellipsis-horizontal"
                size={18}
                color={palette.text}
              />
            </Pressable>
          </View>
        }
      />

      <KeyboardAvoidingView
        behavior={Platform.OS === "ios" ? "padding" : undefined}
        style={styles.flex}
      >
        <ScrollView
          contentContainerStyle={styles.scroll}
          keyboardShouldPersistTaps="handled"
          ref={scrollRef}
          showsVerticalScrollIndicator={false}
        >
          {messages.length === 0 ? (
            <View style={styles.empty}>
              <Text style={styles.emptyTitle}>Empty session</Text>
              <Text style={styles.emptyBody}>
                Type below to start. The transcript will stay with this
                conversation.
              </Text>
            </View>
          ) : (
            <View style={styles.messages}>
              {messages.map((message) => (
                <MessageBubble key={message.id} message={message} />
              ))}
            </View>
          )}
        </ScrollView>

        <View style={{ paddingBottom: Math.max(insets.bottom, 4) }}>
          <Composer
            attachment={attachment}
            autoFocus={detail.messages.length === 0}
            mode={mode}
            onChangeText={setPrompt}
            onOpenAttach={() => setAttachSheet(true)}
            onRemoveAttachment={() => setAttachment(null)}
            onSend={() => send()}
            onVoice={() => setVoiceOpen(true)}
            value={prompt}
          />
          <Text style={styles.modeHint}>
            {mode === "agent"
              ? `Agent · ${model.name} · Return to send`
              : "Terminal · goble · Return to run"}
          </Text>
        </View>
      </KeyboardAvoidingView>

      <ComposerSheet
        mode={mode}
        onClose={() => setAttachSheet(false)}
        onPick={(source) => void attach(source)}
        onSelectMode={setMode}
        visible={attachSheet}
      />
      <ModelMenu
        model={model}
        models={models}
        onClose={() => setModelMenu(false)}
        onSelect={setModel}
        onThinking={setThinking}
        thinking={thinking}
        visible={modelMenu}
      />
      <ConversationMenu
        conversation={conversation}
        onClose={() => setActionsOpen(false)}
        onDelete={() => {
          removeConversation(id);
          setActionsOpen(false);
          router.replace("/");
        }}
        onRename={(title) => renameConversation(id, title)}
        onToggleStar={() =>
          setConversationStarred(id, !conversation.starred)
        }
        visible={actionsOpen}
      />
      </SafeAreaView>
      <CallCard
        onClose={() => setVoiceOpen(false)}
        onExchange={addVoiceExchange}
        visible={voiceOpen}
      />
    </View>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  if (message.role === "user") {
    return (
      <View style={styles.userRow}>
        <View style={styles.userBubble}>
          {message.image ? (
            <Image source={{ uri: message.image }} style={styles.userImage} />
          ) : null}
          <Text style={styles.userText}>{message.text}</Text>
        </View>
      </View>
    );
  }

  if (message.role === "assistant") {
    return <RichText style={styles.assistantText} text={message.text} />;
  }

  if (message.role === "tool") {
    const running = message.status === "running";
    const waiting = message.status === "waiting_approval";
    const mark = running || waiting ? palette.warning : palette.success;
    return (
      <View style={styles.toolRow}>
        <Text style={styles.toolHeader}>
          <Text style={{ color: mark }}>◆ </Text>
          <Text style={styles.toolVerb}>
            {running ? "Running " : waiting ? "Waiting " : "Ran "}
          </Text>
          <Text style={styles.toolSubject}>{message.text}</Text>
        </Text>
        {message.output ? (
          <Text style={styles.toolOutput}>{message.output}</Text>
        ) : null}
      </View>
    );
  }

  return <TerminalBlock message={message} />;
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  safe: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  flex: {
    flex: 1,
  },
  topActions: {
    flexDirection: "row",
    gap: 6,
  },
  newButton: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  scroll: {
    paddingBottom: space.lg,
    paddingHorizontal: space.lg,
    paddingTop: space.md,
  },
  empty: {
    alignItems: "center",
    paddingHorizontal: space.xl,
    paddingTop: space.xxl,
  },
  emptyTitle: {
    ...typography.callout,
    color: palette.text,
  },
  emptyBody: {
    ...typography.caption,
    color: palette.muted,
    marginTop: space.xs,
    textAlign: "center",
  },
  messages: {
    gap: space.xl,
  },
  userRow: {
    alignItems: "flex-end",
  },
  userBubble: {
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: 20,
    borderTopRightRadius: 6,
    borderWidth: StyleSheet.hairlineWidth,
    maxWidth: "86%",
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
  },
  userImage: {
    backgroundColor: palette.raised,
    borderRadius: 12,
    height: 168,
    marginBottom: space.sm,
    width: 168,
  },
  userText: {
    ...typography.body,
    color: palette.text,
  },
  assistantText: {
    ...typography.body,
    color: palette.text,
    fontSize: 15,
    lineHeight: 22,
  },
  toolRow: {
    gap: 5,
  },
  toolHeader: {
    ...typography.mono,
    color: palette.text,
    fontSize: 13,
    lineHeight: 19,
  },
  toolVerb: {
    color: palette.text,
    fontWeight: "600",
  },
  toolSubject: {
    color: palette.command,
  },
  toolOutput: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 12,
    lineHeight: 19,
  },
  modeHint: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 9,
    paddingBottom: 2,
    textAlign: "center",
  },
  pressed: {
    opacity: 0.65,
  },
});
