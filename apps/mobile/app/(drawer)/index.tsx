import { Ionicons } from "@expo/vector-icons";
import { LinearGradient } from "expo-linear-gradient";
import { useRouter } from "expo-router";
import { useEffect, useState } from "react";
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
import { CallCard } from "../../components/call-card";
import { pickAttachment, type AttachmentSource } from "../../lib/attachments";
import { greeting } from "../../lib/format";
import { useOpenDrawer } from "../../lib/navigation";
import {
  loadPreferences,
  subscribeDefaultMode,
} from "../../lib/preferences";
import { palette, radius, space, typography, withAlpha } from "../../lib/theme";
import type { SessionMode, ThinkingLevel } from "../../lib/types";
import { useModelChoices } from "../../lib/use-models";

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
  const { defaultModel, models } = useModelChoices();
  const [model, setModel] = useState(defaultModel);
  const [thinking, setThinking] = useState<ThinkingLevel>("standard");
  const [mode, setMode] = useState<SessionMode>("agent");
  const [prompt, setPrompt] = useState("");
  const [attachment, setAttachment] = useState<string | null>(null);
  const [attachSheet, setAttachSheet] = useState(false);
  const [modelMenu, setModelMenu] = useState(false);
  const [voiceOpen, setVoiceOpen] = useState(false);

  useEffect(() => {
    void loadPreferences().then((preferences) => setMode(preferences.mode));
    return subscribeDefaultMode(setMode);
  }, []);

  useEffect(() => {
    setModel(defaultModel);
  }, [defaultModel.id]);

  function send(text = prompt) {
    const value = text.trim();
    if (!value && !attachment) return;
    const image = attachment;
    const id = `session-${Date.now().toString(36)}`;
    setPrompt("");
    setAttachment(null);
    const query = `prompt=${encodeURIComponent(value)}&mode=${mode}${
      image ? `&image=${encodeURIComponent(image)}` : ""
    }`;
    router.push(`/session/${id}?${query}`);
  }

  async function attach(source: AttachmentSource) {
    setAttachSheet(false);
    const uri = await pickAttachment(source);
    if (uri) setAttachment(uri);
  }

  function pickSuggestion(label: string, value: string) {
    if (label === "Open the last terminal") {
      router.push("/session/pty-resize");
      return;
    }
    setMode("agent");
    setPrompt(value);
  }

  return (
    <View style={styles.root}>
      <SafeAreaView style={styles.safe} edges={["top", "left", "right"]}>
      <TopBar
        model={model}
        onAvatar={() => router.push("/settings")}
        onMenu={openDrawer}
        onModel={() => setModelMenu(true)}
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

            <Text style={styles.greeting}>{greeting()}</Text>
            <Text style={styles.title}>What are we building?</Text>
            <Text style={styles.subtitle}>
              Ask the agent, run a command, or pick up a session from the
              sidebar.
            </Text>
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
          <Composer
            attachment={attachment}
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
      </SafeAreaView>
      <CallCard
        onClose={() => setVoiceOpen(false)}
        visible={voiceOpen}
      />
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
  modeHint: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 9,
    paddingBottom: 2,
    textAlign: "center",
  },
  pressed: {
    opacity: 0.6,
  },
});
