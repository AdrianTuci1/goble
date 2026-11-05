import { Ionicons } from "@expo/vector-icons";
import { useEffect, useState } from "react";
import { Pressable, StyleSheet, Switch, Text, View } from "react-native";
import type { ComponentProps } from "react";

import { PageHeader } from "../../components/page-header";
import { Screen } from "../../components/screen";
import { CallCard } from "../../components/call-card";
import { tapFeedback } from "../../lib/haptics";
import {
  loadVoicePreferences,
  saveVoicePreferences,
  subscribeVoicePreferences,
} from "../../lib/preferences";
import { palette, radius, space, typography, withAlpha } from "../../lib/theme";
import {
  DEFAULT_VOICE_PREFERENCES,
  VOICE_PROVIDERS,
  normalizeVoicePreferences,
  voiceDefaultsFor,
  voiceProvider,
  type VoicePreferences,
  type VoiceProviderId,
} from "../../lib/voice";

type IconName = ComponentProps<typeof Ionicons>["name"];

const providerIcons: Record<VoiceProviderId, IconName> = {
  openai: "sparkles-outline",
  elevenlabs: "mic-outline",
  cartesia: "flash-outline",
  "fish-audio": "fish-outline",
};

export default function VoiceSettingsScreen() {
  const [preferences, setPreferences] = useState<VoicePreferences>(
    DEFAULT_VOICE_PREFERENCES,
  );
  const [callOpen, setCallOpen] = useState(false);

  useEffect(() => {
    void loadVoicePreferences().then(setPreferences);
    return subscribeVoicePreferences(setPreferences);
  }, []);

  function save(next: VoicePreferences) {
    const normalized = normalizeVoicePreferences(next);
    setPreferences(normalized);
    void saveVoicePreferences(normalized);
  }

  function pickProvider(provider: VoiceProviderId) {
    tapFeedback();
    save(voiceDefaultsFor(provider, preferences.speakReplies));
  }

  function pickModel(field: "sttModel" | "ttsModel" | "voice", value: string) {
    tapFeedback();
    save({ ...preferences, [field]: value });
  }

  const provider = voiceProvider(preferences.provider);

  return (
    <View style={styles.root}>
      <Screen>
        <PageHeader
          eyebrow="VOICE"
          right={<Text style={styles.rightLabel}>STT → TTS</Text>}
          title="Talk to Goble"
        />

        <Text style={styles.intro}>
          The mic starts a live conversation: speech to text, the agent turn,
          then the reply read aloud. Pick the provider and models here.
        </Text>

        <Text style={styles.sectionLabel}>PROVIDER</Text>
        <View style={styles.group}>
          {VOICE_PROVIDERS.map((entry, index) => {
            const active = entry.id === preferences.provider;
            return (
              <View key={entry.id}>
                {index > 0 ? <View style={styles.divider} /> : null}
                <Pressable
                  accessibilityLabel={`Use ${entry.name} for voice`}
                  accessibilityRole="button"
                  accessibilityState={{ selected: active }}
                  onPress={() => pickProvider(entry.id)}
                  style={({ pressed }) => [
                    styles.providerRow,
                    active && styles.providerActive,
                    pressed && styles.pressed,
                  ]}
                >
                  <View
                    style={[
                      styles.providerIcon,
                      active && styles.providerIconActive,
                    ]}
                  >
                    <Ionicons
                      color={active ? palette.accent : palette.muted}
                      name={providerIcons[entry.id]}
                      size={18}
                    />
                  </View>
                  <View style={styles.providerText}>
                    <Text style={styles.providerName}>{entry.name}</Text>
                    <Text style={styles.providerDetail}>
                      {entry.description}
                    </Text>
                    <Text style={styles.capability}>
                      {entry.transcribe ? "SPEAK + TRANSCRIBE" : "SPEAK ONLY"}
                    </Text>
                  </View>
                  {active ? (
                    <Ionicons
                      color={palette.accent}
                      name="checkmark-circle"
                      size={19}
                    />
                  ) : (
                    <View style={styles.radio} />
                  )}
                </Pressable>
              </View>
            );
          })}
        </View>

        <Text style={[styles.sectionLabel, styles.spaced]}>MODELS</Text>
        <View style={styles.group}>
          <View style={styles.modelRow}>
            <View style={styles.modelHead}>
              <Text style={styles.rowLabel}>Speech to text</Text>
              <Text style={styles.rowDetail}>What the mic hears.</Text>
            </View>
            {provider.transcribe ? (
              <View style={styles.chips}>
                {provider.sttModels.map((model) => (
                  <Chip
                    active={model === preferences.sttModel}
                    key={model}
                    label={model}
                    onPress={() => pickModel("sttModel", model)}
                  />
                ))}
              </View>
            ) : (
              <Text style={styles.unavailable}>
                {provider.name} is speak only. Pick OpenAI, ElevenLabs or Fish
                Audio to talk to the agent.
              </Text>
            )}
          </View>

          <View style={styles.divider} />

          <View style={styles.modelRow}>
            <View style={styles.modelHead}>
              <Text style={styles.rowLabel}>Text to speech</Text>
              <Text style={styles.rowDetail}>How replies are read aloud.</Text>
            </View>
            <View style={styles.chips}>
              {provider.ttsModels.map((model) => (
                <Chip
                  active={model === preferences.ttsModel}
                  key={model}
                  label={model}
                  onPress={() => pickModel("ttsModel", model)}
                />
              ))}
            </View>
          </View>

          <View style={styles.divider} />

          <View style={styles.modelRow}>
            <View style={styles.modelHead}>
              <Text style={styles.rowLabel}>Voice</Text>
              <Text style={styles.rowDetail}>The timbre of the reply.</Text>
            </View>
            <View style={styles.chips}>
              {provider.voices.map((voice) => (
                <Chip
                  active={voice === preferences.voice}
                  key={voice}
                  label={voice}
                  onPress={() => pickModel("voice", voice)}
                />
              ))}
            </View>
          </View>
        </View>

        <Text style={[styles.sectionLabel, styles.spaced]}>PREFERENCES</Text>
        <View style={styles.group}>
          <View style={styles.switchRow}>
            <View style={styles.switchText}>
              <Text style={styles.rowLabel}>Speak replies aloud</Text>
              <Text style={styles.rowDetail}>
                Read the assistant reply with {preferences.voice} after each
                turn.
              </Text>
            </View>
            <Switch
              ios_backgroundColor={palette.raised}
              onValueChange={(next) => {
                tapFeedback();
                save({ ...preferences, speakReplies: next });
              }}
              thumbColor={preferences.speakReplies ? palette.text : palette.muted}
              trackColor={{ false: palette.raised, true: palette.accentSoft }}
              value={preferences.speakReplies}
            />
          </View>
        </View>

        <Pressable
          accessibilityLabel="Try a voice call"
          accessibilityRole="button"
          onPress={() => {
            tapFeedback();
            setCallOpen(true);
          }}
          style={({ pressed }) => [styles.test, pressed && styles.pressed]}
        >
          <View style={styles.testIcon}>
            <Ionicons color={palette.accent} name="call-outline" size={19} />
          </View>
          <View style={styles.testText}>
            <Text style={styles.testTitle}>Try a voice call</Text>
            <Text style={styles.testBody}>
              {provider.name} · {preferences.sttModel || "no STT"} →{" "}
              {preferences.ttsModel}
            </Text>
          </View>
          <Ionicons name="chevron-forward" size={15} color={palette.subtle} />
        </Pressable>

        <Text style={styles.note}>
          Provider keys stay on the gateway. The phone only chooses the provider
          and models for this workspace.
        </Text>
      </Screen>
      <CallCard onClose={() => setCallOpen(false)} visible={callOpen} />
    </View>
  );
}

function Chip({
  label,
  active,
  onPress,
}: {
  label: string;
  active: boolean;
  onPress: () => void;
}) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected: active }}
      onPress={onPress}
      style={({ pressed }) => [
        styles.chip,
        active && styles.chipActive,
        pressed && styles.pressed,
      ]}
    >
      <Text style={[styles.chipLabel, active && styles.chipLabelActive]}>
        {label}
      </Text>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  rightLabel: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
  },
  intro: {
    ...typography.caption,
    color: palette.muted,
    lineHeight: 18,
    marginBottom: space.xl,
    marginTop: -space.sm,
    maxWidth: 340,
  },
  sectionLabel: {
    ...typography.eyebrow,
    color: palette.muted,
    marginBottom: space.sm,
    paddingLeft: space.xs,
  },
  spaced: {
    marginTop: space.xl,
  },
  group: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: "hidden",
  },
  divider: {
    backgroundColor: palette.border,
    height: StyleSheet.hairlineWidth,
    marginLeft: space.md,
  },
  providerRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.md,
    paddingVertical: space.md,
  },
  providerActive: {
    backgroundColor: withAlpha(palette.accent, 0.05),
  },
  providerIcon: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderRadius: radius.md,
    height: 36,
    justifyContent: "center",
    width: 36,
  },
  providerIconActive: {
    backgroundColor: palette.accentSoft,
  },
  providerText: {
    flex: 1,
    gap: 2,
  },
  providerName: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  providerDetail: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
    lineHeight: 17,
  },
  capability: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 9,
    letterSpacing: 0.5,
    marginTop: 2,
  },
  radio: {
    borderColor: palette.borderStrong,
    borderRadius: 9,
    borderWidth: 1,
    height: 18,
    width: 18,
  },
  modelRow: {
    gap: space.sm,
    padding: space.md,
  },
  modelHead: {
    gap: 1,
  },
  rowLabel: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  rowDetail: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
    lineHeight: 17,
  },
  chips: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  chip: {
    backgroundColor: palette.bg,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  chipActive: {
    backgroundColor: palette.accentSoft,
    borderColor: withAlpha(palette.accent, 0.4),
  },
  chipLabel: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 10,
  },
  chipLabelActive: {
    color: palette.text,
  },
  unavailable: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 18,
  },
  switchRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    paddingHorizontal: space.md,
    paddingVertical: space.md,
  },
  switchText: {
    flex: 1,
    gap: 2,
  },
  test: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.md,
    marginTop: space.xl,
    padding: space.md,
  },
  testIcon: {
    alignItems: "center",
    backgroundColor: palette.accentSoft,
    borderRadius: radius.md,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  testText: {
    flex: 1,
    gap: 1,
  },
  testTitle: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  testBody: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 10,
  },
  note: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
    lineHeight: 17,
    paddingBottom: space.xl,
    paddingTop: space.lg,
  },
  pressed: {
    opacity: 0.7,
  },
});
