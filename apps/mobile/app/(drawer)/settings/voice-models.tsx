import { useLocalSearchParams, useRouter } from "expo-router";

import { ChoicePage } from "../../../components/settings/choice-page";
import { useVoicePreferences } from "../../../lib/use-voice";
import { voiceProvider, type VoicePreferences } from "../../../lib/voice";

type Kind = "stt" | "tts" | "voice";

const copy: Record<
  Kind,
  { detail: string; placeholder: string; title: string }
> = {
  stt: {
    detail: "Transcription model",
    placeholder: "Search transcription models",
    title: "Speech to text",
  },
  tts: {
    detail: "Speech model",
    placeholder: "Search speech models",
    title: "Text to speech",
  },
  voice: {
    detail: "Voice",
    placeholder: "Search voices",
    title: "Voice",
  },
};

function kindOf(value: unknown): Kind {
  return value === "tts" || value === "voice" ? value : "stt";
}

export default function VoiceModelScreen() {
  const router = useRouter();
  const params = useLocalSearchParams<{ kind?: string }>();
  const kind = kindOf(params.kind);
  const { preferences, save } = useVoicePreferences();
  const provider = voiceProvider(preferences.provider);
  const text = copy[kind];

  const options =
    kind === "stt"
      ? provider.sttModels
      : kind === "tts"
        ? provider.ttsModels
        : provider.voices;
  const value =
    kind === "stt"
      ? preferences.sttModel
      : kind === "tts"
        ? preferences.ttsModel
        : preferences.voice;
  const preferred =
    kind === "stt"
      ? provider.defaults.stt
      : kind === "tts"
        ? provider.defaults.tts
        : provider.defaults.voice;

  return (
    <ChoicePage
      footer={`${provider.name} serves these. The keys stay on the gateway; the phone only picks the value.`}
      groups={[
        {
          options: options.map((option) => ({
            description:
              option === preferred
                ? `${provider.name}'s default · ${text.detail.toLowerCase()}`
                : text.detail,
            id: option,
            label: option,
          })),
          title: provider.name,
        },
      ]}
      onSelect={(id) => {
        const next: VoicePreferences = { ...preferences };
        if (kind === "stt") next.sttModel = id;
        else if (kind === "tts") next.ttsModel = id;
        else next.voice = id;
        save(next);
        router.back();
      }}
      placeholder={text.placeholder}
      title={text.title}
      value={value}
    />
  );
}
