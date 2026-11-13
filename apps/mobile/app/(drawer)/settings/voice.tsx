import { Ionicons } from "@expo/vector-icons";
import { useState } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";
import type { ComponentProps, ReactNode } from "react";

import { CallCard } from "../../../components/call-card";
import { SettingsPage } from "../../../components/settings/settings-page";
import { SettingsRow } from "../../../components/settings/settings-row";
import { SettingsSection } from "../../../components/settings/settings-section";
import { SettingsSwitchRow } from "../../../components/settings/settings-switch-row";
import { palette, radius, space, typography, withAlpha } from "../../../lib/theme";
import { useVoicePreferences } from "../../../lib/use-voice";
import {
  VOICE_PROVIDERS,
  voiceDefaultsFor,
  voiceProvider,
  type VoiceProviderId,
} from "../../../lib/voice";

type IconName = ComponentProps<typeof Ionicons>["name"];

const providerIcons: Record<VoiceProviderId, IconName> = {
  openai: "sparkles-outline",
  elevenlabs: "mic-outline",
  cartesia: "flash-outline",
  "fish-audio": "fish-outline",
};

export default function VoiceSettingsScreen() {
  const { preferences, save } = useVoicePreferences();
  const [callOpen, setCallOpen] = useState(false);
  const provider = voiceProvider(preferences.provider);

  function pickProvider(id: VoiceProviderId) {
    save(voiceDefaultsFor(id, preferences.speakReplies));
  }

  function pickModel(
    field: "sttModel" | "ttsModel" | "voice",
    value: string,
  ) {
    save({ ...preferences, [field]: value });
  }

  return (
    <>
      <SettingsPage
        right={
          <Text style={styles.topHint}>
            {preferences.sttModel ? "STT → TTS" : "TTS"}
          </Text>
        }
        title="Voice"
      >
        <SettingsSection
          footer="The mic starts a live conversation: speech to text, the agent turn, then the reply read aloud."
          title="Provider"
        >
          {VOICE_PROVIDERS.map((entry) => (
            <SettingsRow
              description={entry.description}
              detail={entry.transcribe ? "Speak + transcribe" : "Speak only"}
              icon={providerIcons[entry.id]}
              key={entry.id}
              label={entry.name}
              onPress={() => pickProvider(entry.id)}
              selected={entry.id === preferences.provider}
            />
          ))}
        </SettingsSection>

        <SettingsSection title="Models">
          <ModelGroup
            description="What the mic hears."
            label="Speech to text"
          >
            {provider.transcribe ? (
              provider.sttModels.map((model) => (
                <Chip
                  active={model === preferences.sttModel}
                  key={model}
                  label={model}
                  onPress={() => pickModel("sttModel", model)}
                />
              ))
            ) : (
              <Text style={styles.unavailable}>
                {provider.name} is speak only. Pick OpenAI, ElevenLabs or Fish
                Audio to talk to the agent.
              </Text>
            )}
          </ModelGroup>

          <ModelGroup description="How replies are read aloud." label="Text to speech">
            {provider.ttsModels.map((model) => (
              <Chip
                active={model === preferences.ttsModel}
                key={model}
                label={model}
                onPress={() => pickModel("ttsModel", model)}
              />
            ))}
          </ModelGroup>

          <ModelGroup description="The timbre of the reply." label="Voice">
            {provider.voices.map((voice) => (
              <Chip
                active={voice === preferences.voice}
                key={voice}
                label={voice}
                onPress={() => pickModel("voice", voice)}
              />
            ))}
          </ModelGroup>
        </SettingsSection>

        <SettingsSection footer="Provider keys stay on the gateway. The phone only chooses the provider and models for this workspace.">
          <SettingsSwitchRow
            description={`Read the reply with ${preferences.voice} after each turn.`}
            icon="volume-high-outline"
            label="Speak replies aloud"
            onValueChange={(next) => save({ ...preferences, speakReplies: next })}
            value={preferences.speakReplies}
          />
          <SettingsRow
            description={`${provider.name} · ${preferences.ttsModel}`}
            icon="call-outline"
            label="Try a voice call"
            onPress={() => setCallOpen(true)}
          />
        </SettingsSection>
      </SettingsPage>
      <CallCard onClose={() => setCallOpen(false)} visible={callOpen} />
    </>
  );
}

function ModelGroup({
  label,
  description,
  children,
}: {
  label: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <View style={styles.group}>
      <View style={styles.groupHead}>
        <Text style={styles.groupLabel}>{label}</Text>
        <Text style={styles.groupDescription}>{description}</Text>
      </View>
      <View style={styles.chips}>{children}</View>
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
  topHint: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
  },
  group: {
    gap: space.sm,
    padding: space.lg,
  },
  groupHead: {
    gap: 2,
  },
  groupLabel: {
    color: palette.text,
    fontSize: 16,
    lineHeight: 22,
  },
  groupDescription: {
    color: palette.muted,
    fontSize: 14,
    lineHeight: 19,
  },
  chips: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  chip: {
    backgroundColor: palette.bg,
    borderRadius: radius.pill,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  chipActive: {
    backgroundColor: withAlpha(palette.accent, 0.14),
  },
  chipLabel: {
    color: palette.muted,
    fontSize: 13,
    lineHeight: 17,
  },
  chipLabelActive: {
    color: palette.accent,
    fontWeight: "600",
  },
  unavailable: {
    color: palette.subtle,
    fontSize: 13,
    lineHeight: 19,
  },
  pressed: {
    opacity: 0.7,
  },
});
