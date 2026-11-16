import { Ionicons } from "@expo/vector-icons";
import { useFocusEffect, useLocalSearchParams, useRouter } from "expo-router";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  AppState,
  Image,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context";

import {
  Composer,
  ComposerSheet,
  ConversationMenu,
  ModelMenu,
  PulseDot,
  RichText,
  TerminalBlock,
  TerminalSurface,
  TopBar,
  takeAttachment,
  useOpenDrawer,
  demoConversation,
  forgetConversation,
  rememberConversation,
  removeConversation,
  renameConversation,
  setConversationPreview,
  setConversationStarred,
  storeConversationMessages,
  subscribeConversations,
  palette,
  radius,
  space,
  typography,
  withAlpha,
  refreshThreads,
  startTurn,
  useConnection,
  uploadAttachment,
  requestChats,
  requestChat,
  useTerminal,
  useModelChoices,
  type Turn,
  type ChatMessage,
  type RemoteChat,
  type SessionMode,
  type ThinkingLevel,
  type CommandDecision,
  type PickedAttachment,
} from "@goble/common";
import { startDictation, type Dictation } from "../../../lib/dictation";
import {
  pickAttachment,
  type AttachmentSource,
} from "../../../lib/attachments";
import { tapFeedback } from "../../../lib/haptics";

import {
  notifyAgentNeedsHelp,
  notifyRunFinished,
} from "../../../lib/notifications";

import { useVoicePreferences } from "../../../lib/use-voice";
import { voiceProvider } from "../../../lib/voice";
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
    temporary?: string;
  }>();
  const id = params.id ?? "new";
  const initialMode: SessionMode = params.mode === "terminal" ? "terminal" : "agent";
  const temporary = params.temporary === "1";
  const detail = demoConversation(
    id,
    params.prompt,
    initialMode,
    params.image,
    temporary,
  );
  const { defaultModel, models } = useModelChoices();
  const remote = useConnection();
  const terminal = useTerminal(id);
  /** The question a run paused on, and the turn waiting for its answer. */
  const [question, setQuestion] = useState<{
    text: string;
    replies: string[];
  } | null>(null);
  const [answer, setAnswer] = useState("");
  /**
   * The command a run suspended before running, and the line the card would
   * send. The first candidate is what the agent proposed.
   */
  const [proposal, setProposal] = useState<{
    id: string;
    candidates: string[];
    cwd: string;
  } | null>(null);
  const [command, setCommand] = useState("");
  const turnRef = useRef<Turn | null>(null);
  /**
   * A banner is for a turn nobody is watching: the user is on another screen or
   * the app is in the background. A turn that finishes on screen is its own
   * feedback.
   */
  const focusedRef = useRef(true);
  useFocusEffect(
    useCallback(() => {
      focusedRef.current = true;
      return () => {
        focusedRef.current = false;
      };
    }, []),
  );
  const alertsHere = () =>
    !focusedRef.current || AppState.currentState !== "active";

  const [model, setModel] = useState(defaultModel);
  const [thinking, setThinking] = useState<ThinkingLevel>("standard");
  const [mode, setMode] = useState<SessionMode>(detail.conversation.mode);
  const [conversation, setConversation] = useState(detail.conversation);
  const [messages, setMessages] = useState<ChatMessage[]>(detail.messages);
  const [prompt, setPrompt] = useState("");
  const [attachment, setAttachment] = useState<PickedAttachment | null>(null);
  const [attachSheet, setAttachSheet] = useState(false);
  const [modelMenu, setModelMenu] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [dictation, setDictation] = useState<string | null>(null);
  const [muted, setMuted] = useState(false);
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const dictationRef = useRef<Dictation | null>(null);
  const dictationText = useRef("");
  const { preferences: voice } = useVoicePreferences();
  const canDictate = voiceProvider(voice.provider).transcribe;
  const scrollRef = useRef<ScrollViewInstance>(null);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  function commit(next: ChatMessage[]) {
    messagesRef.current = next;
    setMessages(next);
    storeConversationMessages(id, next);
  }

  function appendMessage(message: ChatMessage) {
    commit([...messagesRef.current, message]);
  }

  function patchMessage(
    messageId: string,
    update: (message: ChatMessage) => ChatMessage,
  ) {
    commit(
      messagesRef.current.map((message) =>
        message.id === messageId ? update(message) : message,
      ),
    );
  }

  useEffect(
    () => () => {
      void dictationRef.current?.stop();
      // Leaving the screen stops the stream; the worker keeps the turn.
      turnRef.current?.detach();
      turnRef.current = null;
    },
    [],
  );

  // A shell lives on the worker under the pane id this phone opened, and dies
  // with the relay socket. Open one when the link is live, and open a fresh one
  // after a drop would have taken the old one with it.
  useEffect(() => {
    if (mode !== "terminal" || remote.status !== "live") return;
    terminal.session.open();
  }, [mode, remote.status, terminal.session]);

  // The worker holds the durable copy of a thread. Reading it on arrival is
  // what makes history survive the app being closed; the local transcript only
  // stands in until the worker answers.
  useEffect(() => {
    if (remote.status !== "live" || turnRef.current) return;
    let cancelled = false;
    const show = (chat: RemoteChat | null) => {
      if (!chat || chat.messages.length === 0) return;
      commit(
        chat.messages.map((message) => ({
          at: message.created_at,
          id: message.id,
          role: message.role === "user" ? "user" : "assistant",
          text: message.content,
        })),
      );
    };
    void requestChats()
      .then((chats) => {
        if (cancelled) return;
        const chat = chats.find((item) => item.id === id);
        if (chat) {
          show(chat);
          return;
        }
        // A subagent's child chat is not in the top-level list, so ask the
        // worker for it by id. A brand-new session has nothing stored yet.
        if (id === "new") return;
        return requestChat(id).then((child) => {
          if (!cancelled) show(child);
        });
      })
      .catch(() => {
        // A worker that cannot list conversations keeps the local transcript.
      });
    return () => {
      cancelled = true;
    };
    // `commit` writes through a ref, so the effect keys on the thread and link.
  }, [id, remote.status]);

  // A prompt typed on Home opens this screen. A paired phone runs it on the
  // worker, so the demo transcript that stands in until then is dropped and
  // the turn (with the image that came along) is the only record of it.
  const seededPrompt = useRef(params.prompt);
  useEffect(() => {
    if (remote.status !== "live" || !seededPrompt.current) return;
    const prompt = seededPrompt.current;
    seededPrompt.current = undefined;
    const held = takeAttachment(params.image);
    if (held) setAttachment(held);
    commit([]);
    void send(prompt, held);
  }, [remote.status]);

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
    if (alertsHere()) void notifyAgentNeedsHelp(conversation.title);
  }, [needsHelp, id, conversation.title]);

  useEffect(() => {
    if (!temporary) return;
    return () => forgetConversation(id);
  }, [id, temporary]);

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
  }, [messages, dictation]);

  async function send(text = prompt, attached = attachment) {
    const value = text.trim();
    if (!value && !attached) return;
    const now = new Date().toISOString();
    const image = attached?.uri;
    const userMessage: ChatMessage = {
      id: `user-${Date.now()}`,
      role: "user",
      text: value || "Attached image",
      image,
      at: now,
    };
    const replyId = `reply-${Date.now()}`;
    const reply: ChatMessage = {
      id: replyId,
      role: "assistant",
      text: "This session is running on the demo workspace. Pair with a gateway in Settings and the turn runs on the worker instead.",
      at: now,
    };
    if (remote.status === "live") {
      // A paired phone runs the turn on the worker and streams the answer in;
      // the reply placeholder stands in until the first delta arrives.
      appendMessage(userMessage);
      appendMessage({
        id: replyId,
        role: "assistant",
        text: "",
        at: now,
        status: "running",
      });
      let turnPrompt = value;
      if (attached?.base64) {
        // The worker stores the bytes in its workspace; the prompt names the
        // path so the agent reads the file with the tools it already has.
        try {
          const path = await uploadAttachment(attached.name, attached.base64);
          turnPrompt = `${value}\n\n[Attached image: ${path}]`;
        } catch (error) {
          patchMessage(replyId, (message) => ({
            ...message,
            status: "error",
            text: `The attachment did not reach the worker: ${
              (error as Error).message
            }`,
          }));
          return;
        }
      }
      const turn = startTurn(id, turnPrompt, {
        onDelta: (delta) =>
          patchMessage(replyId, (message) => ({
            ...message,
            status: "running",
            text: message.text + delta,
          })),
        onToolStarted: (call) =>
          appendMessage({
            id: `tool-${call.id}`,
            role: "tool",
            text: call.name,
            at: new Date().toISOString(),
            status: "running",
          }),
        onToolFinished: (call) =>
          patchMessage(`tool-${call.id}`, (message) => ({
            ...message,
            output: call.error ?? call.result ?? message.output,
            status: call.error ? "error" : "done",
          })),
        onAskUser: (text, replies) => {
          setQuestion({ replies, text });
          appendMessage({
            id: `ask-${Date.now()}`,
            role: "tool",
            text,
            at: new Date().toISOString(),
            status: "waiting_approval",
          });
        },
        onCommandProposed: (next) => {
          setProposal(next);
          setCommand(next.candidates[0] ?? "");
          appendMessage({
            id: `command-${next.id}`,
            role: "tool",
            text: next.candidates[0] ?? "Run a command?",
            at: new Date().toISOString(),
            status: "waiting_approval",
          });
        },
        onDone: () => {
          turnRef.current = null;
          setQuestion(null);
          setProposal(null);
          setCommand("");
          patchMessage(replyId, (message) => ({
            ...message,
            status: "done",
            text: message.text || "(the run finished without text)",
          }));
          // The worker stored this turn under the thread id, so the drawer can
          // list the conversation the moment it settles.
          refreshThreads();
          if (alertsHere()) void notifyRunFinished(conversation.title);
        },
        onError: (message) => {
          turnRef.current = null;
          setQuestion(null);
          setProposal(null);
          setCommand("");
          patchMessage(replyId, (replyMessage) => ({
            ...replyMessage,
            status: "error",
            text: replyMessage.text || message,
          }));
        },
        // The screen names the turn in the alert it raises after it is closed.
      }, conversation.title);
      turnRef.current = turn;
      if (!turn) {
        patchMessage(replyId, (message) => ({
          ...message,
          status: "error",
          text: "The worker link is not live.",
        }));
      }
    } else {
      appendMessage(userMessage);
      appendMessage(reply);
    }
    if (messagesRef.current.length <= 2) {
      renameConversation(id, (value || "Attached image").slice(0, 54));
    }
    setConversationPreview(id, value || "Attached image");
    setPrompt("");
    setAttachment(null);
    if (remote.status !== "live" && alertsHere()) {
      void notifyRunFinished(conversation.title);
    }
  }

  /** The wave in the composer: tap to listen, tap again to stop. */
  function toggleDictation() {
    if (dictationRef.current) {
      void finishDictation();
      return;
    }
    if (!canDictate) return;
    dictationText.current = "";
    setMuted(false);
    setVoiceError(null);
    setDictation("");
    dictationRef.current = startDictation({
      provider: voice.provider,
      model: voice.sttModel,
      onPartial: (text) => {
        dictationText.current = text;
        setDictation(text);
      },
      onError: (message) => {
        setVoiceError(message);
        setDictation(null);
        setMuted(false);
        dictationRef.current = null;
      },
    });
  }

  /** Mute holds the microphone; the words pick up where they stopped. */
  function toggleMute() {
    const next = !muted;
    setMuted(next);
    dictationRef.current?.setMuted(next);
  }

  /** Stop listening and leave what was heard in the composer, ready to send. */
  async function finishDictation() {
    const dictation = dictationRef.current;
    dictationRef.current = null;
    setMuted(false);
    if (!dictation) {
      setDictation(null);
      return;
    }
    // The last segment is transcribed before the bar closes, so the final
    // sentence is not lost to the tap that ended the session.
    const heard = (await dictation.stop()).trim() || dictationText.current.trim();
    setDictation(null);
    if (heard) setPrompt(heard);
  }

  async function attach(source: AttachmentSource) {
    setAttachSheet(false);
    const picked = await pickAttachment(source);
    if (picked) setAttachment(picked);
  }

  /** Answer what the run paused on; the worker resumes the same turn. */
  function answerQuestion(text: string) {
    const value = text.trim();
    if (!value) return;
    tapFeedback();
    turnRef.current?.answer(value);
    setQuestion(null);
    setAnswer("");
  }

  /**
   * Decide the command the run suspended on. A line the agent proposed is an
   * approval; a rewritten one is an edit, so the worker runs what the user
   * sees and the transcript records the change.
   */
  function decideCommand(decision: CommandDecision) {
    tapFeedback();
    turnRef.current?.decideCommand(decision);
    // The row the proposal appended stops waiting once it has an answer.
    if (proposal) {
      const rejected = decision.type === "reject";
      patchMessage(`command-${proposal.id}`, (message) => ({
        ...message,
        output: rejected ? decision.payload : message.output,
        status: rejected ? "skipped" : "done",
        text: rejected ? message.text : decision.payload,
      }));
    }
    setProposal(null);
    setCommand("");
  }

  /** Terminal mode is the shell itself; the agent's transcript is the other one. */
  const terminalMode = mode === "terminal";

  return (
    <View style={styles.root}>
      <SafeAreaView style={styles.safe} edges={["top", "left", "right"]}>
      <TopBar
        model={model}
        onMenu={openDrawer}
        onModel={() => setModelMenu(true)}
        title={terminalMode ? "Terminal" : undefined}
        right={
          <View style={styles.topActions}>
            <Pressable
              accessibilityHint="Starts a fresh conversation."
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
              accessibilityHint="Opens star, rename and delete for this conversation."
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
        {terminalMode ? (
          <View style={[styles.flex, { paddingBottom: Math.max(insets.bottom, 8) }]}>
            <TerminalSurface
              onSwitchToAgent={() => setMode("agent")}
              session={terminal.session}
              snapshot={terminal.snapshot}
            />
          </View>
        ) : (
        <>
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
          {dictation !== null ? (
            <View style={styles.liveRow}>
              <View style={styles.liveBubble}>
                <Text style={styles.liveText}>
                  {dictation || "Listening…"}
                </Text>
              </View>
            </View>
          ) : null}
        </ScrollView>

        <View style={{ paddingBottom: Math.max(insets.bottom, 4) }}>
          {proposal ? (
            <View style={styles.askCard} testID="agent-command">
              <View style={styles.askHeader}>
                <Ionicons
                  accessible={false}
                  color={palette.warning}
                  name="terminal-outline"
                  size={15}
                />
                <Text style={styles.askLabel}>Run this command?</Text>
              </View>
              {proposal.candidates.length > 1 ? (
                <View style={styles.askReplies}>
                  {proposal.candidates.map((candidate) => (
                    <Pressable
                      accessibilityHint="Fills the command field with this command."
                      accessibilityLabel={`Use ${candidate}`}
                      accessibilityRole="button"
                      hitSlop={7}
                      key={candidate}
                      onPress={() => {
                        tapFeedback();
                        setCommand(candidate);
                      }}
                      style={({ pressed }) => [
                        styles.cmdCandidate,
                        candidate === command && styles.cmdCandidateActive,
                        pressed && styles.pressed,
                      ]}
                    >
                      <Text numberOfLines={1} style={styles.cmdCandidateText}>
                        {candidate}
                      </Text>
                    </Pressable>
                  ))}
                </View>
              ) : null}
              <TextInput
                accessibilityLabel="Command to run"
                autoCapitalize="none"
                autoCorrect={false}
                onChangeText={setCommand}
                selectionColor={palette.accent}
                style={[styles.askInput, styles.cmdInput]}
                value={command}
              />
              <Text numberOfLines={1} style={styles.cmdCwd}>
                {proposal.cwd}
              </Text>
              <View style={styles.cmdActions}>
                <Pressable
                  accessibilityHint="Runs the command shown in the field."
                  accessibilityLabel="Run command"
                  accessibilityRole="button"
                  accessibilityState={{ disabled: !command.trim() }}
                  disabled={!command.trim()}
                  hitSlop={5}
                  onPress={() =>
                    decideCommand(
                      proposal.candidates.includes(command)
                        ? { type: "approve", payload: command }
                        : { type: "edit", payload: command.trim() },
                    )
                  }
                  style={({ pressed }) => [
                    styles.cmdRun,
                    !command.trim() && styles.cmdRunDisabled,
                    pressed && styles.pressed,
                  ]}
                >
                  <Text style={styles.cmdRunText}>Run</Text>
                </Pressable>
                <Pressable
                  accessibilityHint="Rejects the proposal without running it."
                  accessibilityLabel="Skip command"
                  accessibilityRole="button"
                  hitSlop={5}
                  onPress={() =>
                    decideCommand({
                      type: "reject",
                      payload: "Skipped on the phone.",
                    })
                  }
                  style={({ pressed }) => [
                    styles.cmdSkip,
                    pressed && styles.pressed,
                  ]}
                >
                  <Text style={styles.cmdSkipText}>Skip</Text>
                </Pressable>
              </View>
            </View>
          ) : null}
          {question ? (
            <View style={styles.askCard} testID="agent-question">
              <View style={styles.askHeader}>
                <Ionicons
                  accessible={false}
                  color={palette.warning}
                  name="help-circle-outline"
                  size={15}
                />
                <Text style={styles.askLabel}>Waiting on you</Text>
              </View>
              <Text style={styles.askText}>{question.text}</Text>
              {question.replies.length > 0 ? (
                <View style={styles.askReplies}>
                  {question.replies.map((reply) => (
                    <Pressable
                      accessibilityLabel={`Answer ${reply}`}
                      accessibilityRole="button"
                      hitSlop={7}
                      key={reply}
                      onPress={() => answerQuestion(reply)}
                      style={({ pressed }) => [
                        styles.askReply,
                        pressed && styles.pressed,
                      ]}
                    >
                      <Text style={styles.askReplyText}>{reply}</Text>
                    </Pressable>
                  ))}
                </View>
              ) : null}
              <View style={styles.askInputRow}>
                <TextInput
                  accessibilityLabel="Answer the agent"
                  onChangeText={setAnswer}
                  onSubmitEditing={() => answerQuestion(answer)}
                  placeholder="Type an answer"
                  placeholderTextColor={palette.subtle}
                  selectionColor={palette.accent}
                  style={styles.askInput}
                  value={answer}
                />
                <Pressable
                  accessibilityLabel="Send answer"
                  accessibilityRole="button"
                  accessibilityState={{ disabled: !answer.trim() }}
                  disabled={!answer.trim()}
                  hitSlop={6}
                  onPress={() => answerQuestion(answer)}
                  style={({ pressed }) => [
                    styles.askSend,
                    !answer.trim() && styles.askSendDisabled,
                    pressed && styles.pressed,
                  ]}
                >
                  <Ionicons color={palette.bg} name="arrow-up" size={16} />
                </Pressable>
              </View>
            </View>
          ) : null}
          {conversation.temporary ? (
            <View style={styles.tempNote}>
              <Ionicons
                accessible={false}
                color={palette.accent}
                name="eye-off-outline"
                size={13}
              />
              <Text style={styles.tempNoteText}>
                Temporary · this chat is discarded when you leave
              </Text>
            </View>
          ) : null}
          <Composer
            attachment={attachment?.uri ?? null}
            autoFocus={detail.messages.length === 0}
            dictationText={dictation ?? ""}
            listening={dictation !== null}
            mode={mode}
            muted={muted}
            onChangeText={setPrompt}
            onClose={finishDictation}
            onMute={toggleMute}
            onOpenAttach={() => setAttachSheet(true)}
            onRemoveAttachment={() => setAttachment(null)}
            onSend={() => void send()}
            onVoice={toggleDictation}
            value={prompt}
            voiceDisabled={!canDictate}
          />
          <Text
            style={[styles.modeHint, voiceError ? styles.voiceError : null]}
          >
            {voiceError ??
              (mode === "agent"
                ? `Agent · ${model.name} · Return to send`
                : "Terminal · goble · Return to run")}
          </Text>
        </View>
        </>
        )}
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
    if (message.status === "running" && !message.text) {
      return (
        <View style={styles.workingRow} testID="reply-working">
          <PulseDot color={palette.accent} size={6} />
          <Text style={styles.workingText}>Working…</Text>
        </View>
      );
    }
    return <RichText style={styles.assistantText} text={message.text} />;
  }

  if (message.role === "tool") {
    const running = message.status === "running";
    const waiting = message.status === "waiting_approval";
    const skipped = message.status === "skipped";
    const mark = skipped
      ? palette.subtle
      : running || waiting
        ? palette.warning
        : message.status === "error"
          ? palette.error
          : palette.success;
    return (
      <View style={styles.toolRow}>
        <Text style={styles.toolHeader}>
          <Text style={{ color: mark }}>◆ </Text>
          <Text style={styles.toolVerb}>
            {running
              ? "Running "
              : waiting
                ? "Waiting "
                : skipped
                  ? "Skipped "
                  : "Ran "}
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
  liveRow: {
    alignItems: "flex-end",
    marginTop: space.xl,
  },
  liveBubble: {
    backgroundColor: palette.accentSoft,
    borderColor: withAlpha(palette.accent, 0.45),
    borderRadius: 20,
    borderTopRightRadius: 6,
    borderWidth: StyleSheet.hairlineWidth,
    maxWidth: "86%",
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
  },
  liveText: {
    ...typography.body,
    color: palette.text,
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
  workingRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
  },
  workingText: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 12,
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
  tempNote: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
    justifyContent: "center",
    paddingBottom: space.sm,
    paddingHorizontal: space.lg,
  },
  tempNoteText: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
  },
  modeHint: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 9,
    paddingBottom: 2,
    textAlign: "center",
  },
  voiceError: {
    color: palette.warning,
    fontFamily: undefined,
    fontSize: 11,
    paddingBottom: 2,
  },
  askCard: {
    backgroundColor: palette.warningSoft,
    borderColor: withAlpha(palette.warning, 0.35),
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    gap: space.sm,
    marginBottom: space.sm,
    marginHorizontal: space.lg,
    padding: space.md,
  },
  askHeader: {
    alignItems: "center",
    flexDirection: "row",
    gap: 6,
  },
  askLabel: {
    ...typography.caption,
    color: palette.warning,
    fontSize: 11,
    fontWeight: "700",
    letterSpacing: 0.6,
    textTransform: "uppercase",
  },
  askText: {
    ...typography.body,
    color: palette.text,
    fontSize: 14,
    lineHeight: 20,
  },
  askReplies: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  askReply: {
    backgroundColor: palette.bg,
    borderRadius: radius.pill,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  askReplyText: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
  },
  askInputRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
  },
  askInput: {
    backgroundColor: palette.bg,
    borderRadius: radius.md,
    color: palette.text,
    flex: 1,
    fontSize: 13,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingHorizontal: 12,
    paddingVertical: 9,
  },
  askSend: {
    alignItems: "center",
    backgroundColor: palette.warning,
    borderRadius: radius.md,
    height: 34,
    justifyContent: "center",
    width: 34,
  },
  askSendDisabled: {
    backgroundColor: palette.raised,
  },
  cmdCandidate: {
    backgroundColor: palette.bg,
    borderRadius: radius.sm,
    maxWidth: "100%",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  cmdCandidateActive: {
    backgroundColor: palette.overlay,
  },
  cmdCandidateText: {
    ...typography.mono,
    color: palette.text,
    fontSize: 12,
  },
  cmdInput: {
    fontFamily: typography.mono.fontFamily,
    fontSize: 12.5,
    paddingVertical: 8,
  },
  cmdCwd: {
    ...typography.caption,
    color: palette.subtle,
    fontFamily: typography.mono.fontFamily,
    fontSize: 11,
  },
  cmdActions: {
    flexDirection: "row",
    gap: space.sm,
  },
  cmdRun: {
    alignItems: "center",
    backgroundColor: palette.warning,
    borderRadius: radius.md,
    flex: 1,
    paddingVertical: 9,
  },
  cmdRunDisabled: {
    backgroundColor: palette.raised,
  },
  cmdRunText: {
    ...typography.callout,
    color: palette.bg,
  },
  cmdSkip: {
    alignItems: "center",
    borderColor: palette.border,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 18,
    paddingVertical: 9,
  },
  cmdSkipText: {
    ...typography.callout,
    color: palette.muted,
  },
  pressed: {
    opacity: 0.65,
  },
});
