import { useRouter } from "expo-router";

import { SettingsPage } from "../../../components/settings/settings-page";
import {
  SettingsRow,
  type SettingsIconName,
} from "../../../components/settings/settings-row";
import { SettingsSection } from "../../../components/settings/settings-section";
import { SettingsSwitchRow } from "../../../components/settings/settings-switch-row";
import { useVoicePreferences } from "../../../lib/use-voice";
import {
  VOICE_PROVIDERS,
  voiceDefaultsFor,
  voiceProvider,
  type VoiceProviderId,
} from "../../../lib/voice";

const providerIcons: Record<VoiceProviderId, SettingsIconName> = {
  openai: "sparkles-outline",
  elevenlabs: "mic-outline",
  cartesia: "flash-outline",
  "fish-audio": "fish-outline",
};

export default function VoiceSettingsScreen() {
  const router = useRouter();
  const { preferences, save } = useVoicePreferences();
  const provider = voiceProvider(preferences.provider);
  const defaultNote = (value: string, fallback: string) =>
    value === fallback ? ` · ${provider.name}'s default` : "";

  return (
    <SettingsPage title="Voice">
      <SettingsSection
        footer="The wave in the composer dictates: the words stream into the chat as you speak, and the reply is read aloud when the turn ends."
        title="Provider"
      >
        {VOICE_PROVIDERS.map((entry) => (
          <SettingsRow
            chevron={false}
            description={entry.description}
            icon={providerIcons[entry.id] ?? "mic-outline"}
            key={entry.id}
            label={entry.name}
            onPress={() => save(voiceDefaultsFor(entry.id, preferences.speakReplies))}
            selected={entry.id === preferences.provider}
          />
        ))}
      </SettingsSection>

      <SettingsSection
        footer={
          provider.transcribe
            ? "The model the wave hears with."
            : `${provider.name} cannot transcribe. Pick OpenAI, ElevenLabs or Fish Audio to talk to the agent.`
        }
        title="Dictation"
      >
        <SettingsRow
          chevron
          description={
            provider.transcribe
              ? `${preferences.sttModel}${defaultNote(preferences.sttModel, provider.defaults.stt)}`
              : "No transcription model"
          }
          disabled={!provider.transcribe}
          icon="ear-outline"
          label="Speech to text"
          onPress={() =>
            router.push({
              pathname: "/settings/voice-models",
              params: { kind: "stt" },
            })
          }
        />
      </SettingsSection>

      <SettingsSection
        footer="Provider keys stay on the gateway. The phone only chooses the provider and models for this workspace."
        title="Speech"
      >
        <SettingsRow
          chevron
          description={`${preferences.ttsModel}${defaultNote(preferences.ttsModel, provider.defaults.tts)}`}
          icon="volume-high-outline"
          label="Text to speech"
          onPress={() =>
            router.push({
              pathname: "/settings/voice-models",
              params: { kind: "tts" },
            })
          }
        />
        <SettingsRow
          chevron
          description={`${preferences.voice} · ${provider.name}`}
          icon="person-outline"
          label="Voice"
          onPress={() =>
            router.push({
              pathname: "/settings/voice-models",
              params: { kind: "voice" },
            })
          }
        />
        <SettingsSwitchRow
          description="Read every reply aloud as the turn finishes."
          icon="chatbubble-ellipses-outline"
          label="Speak replies aloud"
          onValueChange={(next) => save({ ...preferences, speakReplies: next })}
          value={preferences.speakReplies}
        />
      </SettingsSection>
    </SettingsPage>
  );
}
