import { Ionicons } from "@expo/vector-icons";
import { usePathname, useRouter } from "expo-router";
import type { ComponentProps } from "react";
import { useEffect, useRef, useState } from "react";
import { Animated, Pressable, StyleSheet, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { Avatar } from "./avatar";
import { BrandMark } from "./brand-mark";
import { tapFeedback } from "../lib/haptics";
import {
  loadVoicePreferences,
  subscribeVoicePreferences,
} from "../lib/preferences";
import { palette, space, withAlpha } from "../lib/theme";
import {
  DEFAULT_VOICE_PREFERENCES,
  voiceProvider,
  type VoicePreferences,
} from "../lib/voice";

type Phase = "listening" | "thinking" | "speaking";
type IconName = ComponentProps<typeof Ionicons>["name"];

interface Exchange {
  role: "user" | "assistant";
  text: string;
}

const BARS = [9, 15, 21, 15, 9];
const TRANSCRIPT_LINES = 4;

const SCRIPT: { heard: string; reply: string }[] = [
  {
    heard: "Walk me through the gateway pairing flow.",
    reply:
      "Pairing starts with a short device code. The phone exchanges it for a device token, then asks the gateway for a sandbox token. The worker only accepts that token while the sandbox is alive.",
  },
  {
    heard: "What changed in the mobile shell?",
    reply:
      "The composer sends on Return. The mic opens a live voice call, and the speech models live under Settings, then Voice.",
  },
  {
    heard: "Add a review step before the migration.",
    reply:
      "Noted. I would pause the migration for an approval and show the diff before it runs.",
  },
];

/** Rakazo-style floating call card; the rest of the app stays interactive. */
export function CallCard({
  visible,
  onClose,
  onExchange,
}: {
  visible: boolean;
  onClose: () => void;
  onExchange?: (exchange: { heard: string; reply: string }) => void;
}) {
  const router = useRouter();
  const pathname = usePathname();
  const insets = useSafeAreaInsets();
  const [preferences, setPreferences] = useState<VoicePreferences>(
    DEFAULT_VOICE_PREFERENCES,
  );
  const [phase, setPhase] = useState<Phase>("listening");
  const [caption, setCaption] = useState("");
  const [muted, setMuted] = useState(false);
  const [transcriptOpen, setTranscriptOpen] = useState(false);
  const [cycle, setCycle] = useState(0);
  const [exchanges, setExchanges] = useState<Exchange[]>([]);
  const exchangeRef = useRef(onExchange);
  exchangeRef.current = onExchange;

  const provider = voiceProvider(preferences.provider);
  const canListen = provider.transcribe;
  const script = SCRIPT[cycle % SCRIPT.length]!;

  useEffect(() => {
    if (!visible) return;
    setExchanges([]);
    setCycle(0);
    setMuted(false);
    setTranscriptOpen(false);
    setPhase("listening");
    setCaption("");
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    void loadVoicePreferences().then(setPreferences);
    return subscribeVoicePreferences(setPreferences);
  }, [visible]);

  useEffect(() => {
    if (!visible || muted || !canListen) return;
    setPhase("listening");
    setCaption("");
    const think = setTimeout(() => {
      setPhase("thinking");
      setCaption(script.heard);
    }, 1800);
    const speak = setTimeout(() => {
      setPhase("speaking");
      setCaption("");
      setExchanges((current) => [
        ...current,
        { role: "user", text: script.heard },
        { role: "assistant", text: script.reply },
      ]);
      exchangeRef.current?.({ heard: script.heard, reply: script.reply });
    }, 3300);
    const settle = setTimeout(() => {
      setPhase("listening");
      setCaption("");
    }, 8200);
    return () => {
      clearTimeout(think);
      clearTimeout(speak);
      clearTimeout(settle);
    };
  }, [visible, muted, canListen, cycle, script.heard, script.reply]);

  if (!visible) return null;

  const silence = muted || !canListen;

  return (
    <View
      accessibilityLabel={`Goble voice call · ${silence ? "muted" : phase}`}
      style={[styles.card, { top: insets.top + 52 }]}
      testID="call-card"
    >
      <View style={styles.row}>
        <BrandMark size={28} />
        <Text numberOfLines={1} style={styles.name}>
          Goble
        </Text>
        <Waveform phase={phase} still={silence || phase === "thinking"} />
        <Avatar size={28} />
      </View>

      <View style={styles.actions}>
        <RoundButton
          label="Voice settings"
          onPress={() => {
            tapFeedback();
            if (!pathname.endsWith("/settings/voice"))
              router.push("/settings/voice");
          }}
        />
        <RoundButton
          icon="chatbox-ellipses-outline"
          label="Transcript"
          onPress={() => {
            tapFeedback();
            setTranscriptOpen((current) => !current);
          }}
          selected={transcriptOpen}
        />
        <RoundButton
          icon={muted ? "mic-off-outline" : "mic-outline"}
          label={muted ? "Unmute" : "Mute"}
          onPress={() => {
            tapFeedback();
            setMuted((current) => !current);
          }}
          selected={muted}
        />
        <RoundButton
          destructive
          icon="call"
          label="Hang up"
          onPress={() => {
            tapFeedback();
            onClose();
          }}
          rotate
        />
      </View>

      {transcriptOpen ? (
        <View style={styles.transcript}>
          {exchanges.slice(-TRANSCRIPT_LINES).map((exchange, index) => (
            <Text
              key={`${exchange.role}-${exchanges.length}-${index}`}
              numberOfLines={1}
              style={[
                styles.line,
                exchange.role === "assistant" ? styles.botLine : styles.userLine,
              ]}
            >
              {exchange.text}
            </Text>
          ))}
          {caption ? (
            <Text numberOfLines={1} style={[styles.line, styles.userLine]}>
              {caption}
            </Text>
          ) : null}
          {exchanges.length === 0 && !caption ? (
            <Text numberOfLines={1} style={[styles.line, styles.userLine]}>
              {!canListen
                ? `${provider.name} can read replies but cannot transcribe speech.`
                : "Listening."}
            </Text>
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

function RoundButton({
  icon = "settings-outline",
  label,
  destructive = false,
  selected = false,
  rotate = false,
  onPress,
}: {
  icon?: IconName;
  label: string;
  destructive?: boolean;
  selected?: boolean;
  rotate?: boolean;
  onPress: () => void;
}) {
  const tone = destructive
    ? palette.white
    : selected
      ? palette.text
      : palette.muted;
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      accessibilityState={{ selected }}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        selected && styles.buttonSelected,
        destructive && styles.buttonDestructive,
        pressed && styles.pressed,
      ]}
    >
      <Ionicons
        color={tone}
        name={icon}
        size={destructive ? 17 : 18}
        style={rotate ? styles.rotated : undefined}
      />
    </Pressable>
  );
}

function Waveform({ phase, still }: { phase: Phase; still: boolean }) {
  const values = useRef(BARS.map(() => new Animated.Value(0.35))).current;

  useEffect(() => {
    if (still) {
      for (const value of values) value.setValue(0.35);
      return;
    }
    const duration = phase === "speaking" ? 300 : 600;
    const loops = values.map((value, index) =>
      Animated.loop(
        Animated.sequence([
          Animated.delay(index * 90),
          Animated.timing(value, {
            duration,
            toValue: 1,
            useNativeDriver: true,
          }),
          Animated.timing(value, {
            duration,
            toValue: 0.35,
            useNativeDriver: true,
          }),
        ]),
      ),
    );
    for (const loop of loops) loop.start();
    return () => {
      for (const loop of loops) loop.stop();
    };
  }, [phase, still, values]);

  return (
    <View accessible={false} style={styles.waveform}>
      {BARS.map((height, index) => (
        <Animated.View
          key={`bar-${index + 1}`}
          style={{
            backgroundColor: still ? palette.subtle : palette.text,
            borderRadius: 2,
            height,
            transform: [{ scaleY: values[index] ?? 1 }],
            width: 3,
          }}
        />
      ))}
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    alignSelf: "center",
    backgroundColor: palette.overlay,
    borderColor: withAlpha(palette.white, 0.08),
    borderRadius: 18,
    borderWidth: StyleSheet.hairlineWidth,
    elevation: 6,
    padding: 12,
    position: "absolute",
    shadowColor: palette.black,
    shadowOffset: { height: 4, width: 0 },
    shadowOpacity: 0.18,
    shadowRadius: 12,
    width: 320,
    zIndex: 50,
  },
  row: {
    alignItems: "center",
    flexDirection: "row",
    gap: 10,
  },
  name: {
    color: palette.text,
    fontSize: 15,
    fontWeight: "500",
    maxWidth: 110,
  },
  waveform: {
    alignItems: "center",
    flex: 1,
    flexDirection: "row",
    gap: 3,
    justifyContent: "center",
  },
  actions: {
    flexDirection: "row",
    gap: space.md,
    justifyContent: "center",
    marginTop: space.md,
  },
  button: {
    alignItems: "center",
    borderColor: palette.borderStrong,
    borderRadius: 22,
    borderWidth: StyleSheet.hairlineWidth,
    height: 44,
    justifyContent: "center",
    width: 44,
  },
  buttonSelected: {
    backgroundColor: withAlpha(palette.white, 0.08),
    borderColor: withAlpha(palette.white, 0.18),
  },
  buttonDestructive: {
    backgroundColor: palette.error,
    borderColor: palette.error,
  },
  rotated: {
    transform: [{ rotate: "135deg" }],
  },
  transcript: {
    borderTopColor: palette.border,
    borderTopWidth: StyleSheet.hairlineWidth,
    gap: 5,
    marginTop: space.md,
    paddingTop: 10,
  },
  line: {
    fontSize: 13,
    lineHeight: 18,
  },
  botLine: {
    color: palette.text,
  },
  userLine: {
    color: palette.muted,
  },
  pressed: {
    opacity: 0.7,
  },
});
