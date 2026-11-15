import { Ionicons, MaterialCommunityIcons } from "@expo/vector-icons";
import { LinearGradient } from "expo-linear-gradient";
import { useRouter } from "expo-router";
import { useEffect, useRef, useState } from "react";
import {
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { SafeAreaView, useSafeAreaInsets } from "react-native-safe-area-context";

import { Composer } from "../../components/composer";
import { ComposerSheet } from "../../components/composer-sheet";
import { ModelMenu } from "../../components/model-menu";
import { TopBar } from "../../components/top-bar";
import {
  pickAttachment,
  type AttachmentSource,
  type PickedAttachment,
} from "../../lib/attachments";
import { startDictation, type Dictation } from "../../lib/dictation";
import { greeting } from "../../lib/format";
import { tapFeedback } from "../../lib/haptics";
import { useOpenDrawer } from "../../lib/navigation";
import { holdAttachment } from "../../lib/pending-attachment";
import {
  loadPreferences,
  subscribeDefaultMode,
} from "../../lib/preferences";
import { palette, radius, space, typography, withAlpha } from "../../lib/theme";
import type { SessionMode, ThinkingLevel } from "../../lib/types";
import { useModelChoices } from "../../lib/use-models";
import { useVoicePreferences } from "../../lib/use-voice";
import { voiceProvider } from "../../lib/voice";

const suggestions = [
  {
    icon: "git-compare-outline" as const,
    label: "Review the gateway diff",
    prompt: "Review the gateway diff and tell me what changed.",
  },
  {
    icon: "cube-outline" as const,
    label: "Start a sandbox run",
    prompt: "Start a sandbox run for the mobile shell and show me the result.",
  },
];

export default function HomeScreen() {
  const router = useRouter();
  const insets = useSafeAreaInsets();
  const openDrawer = useOpenDrawer();
  const { defaultModel, models, preferences } = useModelChoices();
  const [model, setModel] = useState(defaultModel);
  const [thinking, setThinking] = useState<ThinkingLevel>(
    preferences.defaultThinking,
  );
  const [mode, setMode] = useState<SessionMode>("agent");
  const [prompt, setPrompt] = useState("");
  const [temporary, setTemporary] = useState(false);
  const [attachment, setAttachment] = useState<PickedAttachment | null>(null);
  const [attachSheet, setAttachSheet] = useState(false);
  const [modelMenu, setModelMenu] = useState(false);
  const [dictation, setDictation] = useState(false);
  const [muted, setMuted] = useState(false);
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const dictationRef = useRef<Dictation | null>(null);
  const { preferences: voice } = useVoicePreferences();
  const canDictate = voiceProvider(voice.provider).transcribe;

  useEffect(() => () => void dictationRef.current?.stop(), []);

  useEffect(() => {
    void loadPreferences().then((preferences) => setMode(preferences.mode));
    return subscribeDefaultMode(setMode);
  }, []);

  useEffect(() => {
    setModel(defaultModel);
  }, [defaultModel.id]);

  useEffect(() => {
    setThinking(preferences.defaultThinking);
  }, [preferences.defaultThinking]);

  function send(text = prompt) {
    const value = text.trim();
    if (!value && !attachment) return;
    // The session uploads the bytes; the route only has room for the preview.
    if (attachment) holdAttachment(attachment);
    const image = attachment?.uri;
    const id = `session-${Date.now().toString(36)}`;
    setPrompt("");
    setAttachment(null);
    const query = `prompt=${encodeURIComponent(value)}&mode=${mode}${
      image ? `&image=${encodeURIComponent(image)}` : ""
    }${temporary ? "&temporary=1" : ""}`;
    setTemporary(false);
    router.push(`/session/${id}?${query}`);
  }

  async function attach(source: AttachmentSource) {
    setAttachSheet(false);
    const picked = await pickAttachment(source);
    if (picked) setAttachment(picked);
  }

  function pickSuggestion(label: string, value: string) {
    if (label === "Open the last terminal") {
      router.push("/session/pty-resize");
      return;
    }
    setMode("agent");
    setPrompt(value);
  }

  /**
   * The home screen has no transcript to stream into, so the words land in the
   * composer as they are heard and the dictation bar shows them.
   */
  function toggleDictation() {
    if (dictationRef.current) {
      void stopDictation();
      return;
    }
    if (!canDictate) return;
    setMuted(false);
    setVoiceError(null);
    setDictation(true);
    dictationRef.current = startDictation({
      provider: voice.provider,
      model: voice.sttModel,
      onPartial: (heard) => setPrompt(heard),
      onError: (message) => {
        setVoiceError(message);
        setDictation(false);
        setMuted(false);
        dictationRef.current = null;
      },
    });
  }

  /** Stop listening; the last segment is transcribed before the bar closes. */
  async function stopDictation() {
    const dictation = dictationRef.current;
    dictationRef.current = null;
    setDictation(false);
    setMuted(false);
    if (!dictation) return;
    const heard = (await dictation.stop()).trim();
    if (heard) setPrompt(heard);
  }

  function toggleMute() {
    const next = !muted;
    setMuted(next);
    dictationRef.current?.setMuted(next);
  }

  return (
    <View style={styles.root}>
      <SafeAreaView style={styles.safe} edges={["top", "left", "right"]}>
      <TopBar
        model={model}
        onMenu={openDrawer}
        onModel={() => setModelMenu(true)}
        right={
          <Pressable
            accessibilityLabel={
              temporary ? "Turn off temporary chat" : "Start a temporary chat"
            }
            accessibilityRole="button"
            accessibilityState={{ selected: temporary }}
            hitSlop={8}
            onPress={() => {
              tapFeedback();
              setTemporary(!temporary);
            }}
            style={({ pressed }) => [
              styles.tempButton,
              temporary && styles.tempButtonActive,
              pressed && styles.pressed,
            ]}
            testID="temporary-chat"
          >
            <MaterialCommunityIcons
              color={temporary ? palette.accent : palette.text}
              name="incognito"
              size={20}
            />
          </Pressable>
        }
      />

      <KeyboardAvoidingView
        behavior={Platform.OS === "ios" ? "padding" : undefined}
        style={styles.flex}
      >
        <ScrollView
          contentContainerStyle={styles.scroll}
          keyboardShouldPersistTaps="handled"
          showsVerticalScrollIndicator={false}
        >
          <View style={styles.center}>
            {/* Dictation clears the screen: the gradient is the only thing
                moving while the phone listens. */}
            {dictation ? null : (
              <>
                <Text style={styles.greeting}>{greeting()}</Text>
                <Text style={styles.title}>What are we building?</Text>
                <Text style={styles.subtitle}>
                  Ask the agent, run a command, or pick up a session from the
                  sidebar.
                </Text>
              </>
            )}
          </View>

          <View style={styles.suggestions}>
            {suggestions.map((suggestion) => (
              <Pressable
                accessibilityRole="button"
                key={suggestion.label}
                onPress={() =>
                  pickSuggestion(suggestion.label, suggestion.prompt)
                }
                style={({ pressed }) => [
                  styles.suggestion,
                  pressed && styles.pressed,
                ]}
              >
                <Ionicons
                  name={suggestion.icon}
                  size={18}
                  color={palette.muted}
                />
                <Text style={styles.suggestionLabel}>{suggestion.label}</Text>
              </Pressable>
            ))}
            <Pressable
              accessibilityRole="button"
              onPress={() => pickSuggestion("Open the last terminal", "")}
              style={({ pressed }) => [
                styles.suggestion,
                pressed && styles.pressed,
              ]}
            >
              <Ionicons name="terminal-outline" size={18} color={palette.muted} />
              <Text style={styles.suggestionLabel}>Open the last terminal</Text>
              <View style={styles.recentChip}>
                <Text style={styles.recentChipText}>PTY</Text>
              </View>
            </Pressable>
          </View>
        </ScrollView>

        <LinearGradient
          colors={["rgba(14,14,14,0)", withAlpha(palette.accent, 0.09)]}
          pointerEvents="none"
          style={styles.bottomGlow}
        />

        <View style={{ paddingBottom: Math.max(insets.bottom, 4) }}>
          {temporary ? (
            <View style={styles.tempNote}>
              <Ionicons
                color={palette.accent}
                name="eye-off-outline"
                size={13}
              />
              <Text style={styles.tempNoteText}>
                Temporary · this chat stays out of the sidebar
              </Text>
            </View>
          ) : null}
          <Composer
            attachment={attachment?.uri ?? null}
            dictationText={dictation ? prompt : ""}
            listening={dictation}
            mode={mode}
            muted={muted}
            onChangeText={setPrompt}
            onClose={stopDictation}
            onMute={toggleMute}
            onOpenAttach={() => setAttachSheet(true)}
            onRemoveAttachment={() => setAttachment(null)}
            onSend={() => send()}
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
      </SafeAreaView>
    </View>
  );
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
  scroll: {
    flexGrow: 1,
    paddingHorizontal: space.lg,
    paddingTop: space.xxl,
  },
  center: {
    alignItems: "center",
    flex: 1,
    justifyContent: "center",
    paddingBottom: space.xl,
  },
  greeting: {
    ...typography.body,
    color: palette.muted,
    fontSize: 15,
    marginBottom: space.xs,
  },
  title: {
    color: palette.text,
    fontSize: 34,
    fontWeight: "400",
    letterSpacing: -1.2,
    lineHeight: 40,
    textAlign: "center",
  },
  subtitle: {
    ...typography.caption,
    color: palette.subtle,
    lineHeight: 18,
    marginTop: space.md,
    maxWidth: 290,
    textAlign: "center",
  },
  suggestions: {
    gap: 2,
    marginBottom: space.xl,
    marginTop: space.lg,
  },
  suggestion: {
    alignItems: "center",
    borderRadius: radius.md,
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.sm,
    paddingVertical: 11,
  },
  suggestionLabel: {
    ...typography.body,
    color: palette.muted,
    flex: 1,
    fontSize: 14,
  },
  recentChip: {
    backgroundColor: palette.raised,
    borderRadius: radius.sm,
    paddingHorizontal: 5,
    paddingVertical: 2,
  },
  recentChipText: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 9,
  },
  bottomGlow: {
    bottom: 0,
    height: 190,
    left: 0,
    position: "absolute",
    right: 0,
  },
  tempButton: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    height: 36,
    justifyContent: "center",
    width: 36,
  },
  tempButtonActive: {
    backgroundColor: withAlpha(palette.accent, 0.16),
    borderColor: withAlpha(palette.accent, 0.55),
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
  pressed: {
    opacity: 0.6,
  },
});
