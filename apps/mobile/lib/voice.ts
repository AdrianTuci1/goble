export type VoiceProviderId =
  | "openai"
  | "elevenlabs"
  | "cartesia"
  | "fish-audio";

export interface VoiceProviderEntry {
  id: VoiceProviderId;
  name: string;
  description: string;
  /** The provider can transcribe speech, so the mic can open a conversation. */
  transcribe: boolean;
  sttModels: string[];
  ttsModels: string[];
  voices: string[];
  defaults: { stt: string; tts: string; voice: string };
}

export interface VoicePreferences {
  provider: VoiceProviderId;
  sttModel: string;
  ttsModel: string;
  voice: string;
  speakReplies: boolean;
}

export const VOICE_PROVIDERS: VoiceProviderEntry[] = [
  {
    id: "openai",
    name: "OpenAI",
    description: "Simple speech plus Whisper-class transcription.",
    transcribe: true,
    sttModels: ["gpt-4o-mini-transcribe", "whisper-1"],
    ttsModels: ["gpt-4o-mini-tts", "tts-1"],
    voices: ["Alloy", "Ash", "Ballad", "Coral", "Echo", "Sage", "Shimmer"],
    defaults: {
      stt: "gpt-4o-mini-transcribe",
      tts: "gpt-4o-mini-tts",
      voice: "Alloy",
    },
  },
  {
    id: "elevenlabs",
    name: "ElevenLabs",
    description: "Highest quality and cloning. Flash for live calls.",
    transcribe: true,
    sttModels: ["scribe_v1"],
    ttsModels: [
      "eleven_flash_v2_5",
      "eleven_turbo_v2_5",
      "eleven_multilingual_v2",
    ],
    voices: ["Rachel", "Adam", "Bella", "Antoni", "Elli"],
    defaults: {
      stt: "scribe_v1",
      tts: "eleven_flash_v2_5",
      voice: "Rachel",
    },
  },
  {
    id: "cartesia",
    name: "Cartesia",
    description: "Lowest-latency Sonic voices for interruptible calls.",
    transcribe: false,
    sttModels: [],
    ttsModels: ["sonic-2", "sonic-turbo"],
    voices: ["Katie", "Brooke", "Oliver", "Sonic"],
    defaults: {
      stt: "",
      tts: "sonic-2",
      voice: "Katie",
    },
  },
  {
    id: "fish-audio",
    name: "Fish Audio",
    description: "Voice models, cloning and expressive speech.",
    transcribe: true,
    sttModels: ["transcribe-1-pro"],
    ttsModels: ["s2.1-pro", "speech-1.6", "speech-1.5"],
    voices: ["Default", "Sarah", "Emily", "Michael"],
    defaults: {
      stt: "transcribe-1-pro",
      tts: "s2.1-pro",
      voice: "Default",
    },
  },
];

export const DEFAULT_VOICE_PREFERENCES: VoicePreferences = {
  provider: "openai",
  sttModel: "gpt-4o-mini-transcribe",
  ttsModel: "gpt-4o-mini-tts",
  voice: "Alloy",
  speakReplies: true,
};

export function voiceProvider(id: string): VoiceProviderEntry {
  return (
    VOICE_PROVIDERS.find((provider) => provider.id === id) ??
    VOICE_PROVIDERS[0]!
  );
}

/** Keep a stored preference inside the catalog for its selected provider. */
export function normalizeVoicePreferences(
  value?: Partial<VoicePreferences> | null,
): VoicePreferences {
  const provider = voiceProvider(value?.provider ?? "");
  const pick = (candidate: string | undefined, options: string[], fallback: string) =>
    candidate && options.includes(candidate) ? candidate : fallback;
  return {
    provider: provider.id,
    sttModel: pick(value?.sttModel, provider.sttModels, provider.defaults.stt),
    ttsModel: pick(value?.ttsModel, provider.ttsModels, provider.defaults.tts),
    voice: pick(value?.voice, provider.voices, provider.defaults.voice),
    speakReplies: value?.speakReplies ?? true,
  };
}

/** Switch provider and take that provider's defaults for models and voice. */
export function voiceDefaultsFor(
  providerId: VoiceProviderId,
  speakReplies = true,
): VoicePreferences {
  const provider = voiceProvider(providerId);
  return {
    provider: provider.id,
    sttModel: provider.defaults.stt,
    ttsModel: provider.defaults.tts,
    voice: provider.defaults.voice,
    speakReplies,
  };
}
