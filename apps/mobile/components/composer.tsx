import { Ionicons } from "@expo/vector-icons";
import { LinearGradient } from "expo-linear-gradient";
import { useEffect, useRef, useState } from "react";
import {
  Animated,
  Image,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { tapFeedback } from "../lib/haptics";
import { palette, space, typography, withAlpha } from "../lib/theme";
import type { SessionMode } from "../lib/types";

export function Composer({
  mode,
  value,
  attachment,
  onChangeText,
  onSend,
  onOpenAttach,
  onRemoveAttachment,
  onVoice,
  listening = false,
  voiceDisabled = false,
  dictationText = "",
  muted = false,
  onMute,
  onClose,
  placeholder,
  autoFocus = false,
}: {
  mode: SessionMode;
  value: string;
  attachment?: string | null;
  onChangeText: (value: string) => void;
  onSend: () => void;
  onOpenAttach: () => void;
  onRemoveAttachment?: () => void;
  onVoice: () => void;
  /** The phone is dictating, so the composer is the dictation bar. */
  listening?: boolean;
  /** The chosen provider cannot transcribe, so the wave has nothing to open. */
  voiceDisabled?: boolean;
  /** What has been heard so far, drawn inside the bar. */
  dictationText?: string;
  /** The microphone is muted; the words pause until it is turned back on. */
  muted?: boolean;
  onMute?: () => void;
  /** Ends dictation and keeps what was heard. */
  onClose?: () => void;
  placeholder?: string;
  autoFocus?: boolean;
}) {
  const canSend = value.trim().length > 0 || Boolean(attachment);
  const terminal = mode === "terminal";
  /** The input moves the controls below itself once the text wraps. */
  const [expanded, setExpanded] = useState(false);
  const [measuredHeight, setMeasuredHeight] = useState(0);
  const singleLineHeight = useRef(0);
  const inlineTextWidth = useRef(240);

  // Sending clears the draft from the parent, so the controls return inline.
  useEffect(() => {
    if (!value) setExpanded(false);
  }, [value]);

  function change(next: string) {
    onChangeText(next);
    if (!next) setExpanded(false);
  }

  function submit() {
    if (!canSend) return;
    tapFeedback();
    onSend();
  }

  function measure(height: number) {
    setMeasuredHeight(height);
    if (singleLineHeight.current === 0 || value.length === 0) {
      singleLineHeight.current = height;
      if (expanded) setExpanded(false);
      return;
    }
    const limit =
      singleLineHeight.current + Math.max(8, singleLineHeight.current * 0.3);
    if (!expanded) {
      if (height > limit) setExpanded(true);
      return;
    }
    if (height > limit) return;
    const capacity = Math.floor(Math.max(120, inlineTextWidth.current) / 8.2);
    if (value.trim().length <= capacity) setExpanded(false);
  }

  if (listening) {
    return (
      <View style={styles.wrap}>
        <DictationBar
          muted={muted}
          onClose={() => {
            tapFeedback();
            (onClose ?? onVoice)();
          }}
          onMute={() => {
            tapFeedback();
            (onMute ?? onVoice)();
          }}
          onOpenAttach={onOpenAttach}
          text={dictationText}
        />
      </View>
    );
  }

  return (
    <View style={styles.wrap}>
      <View style={[styles.pill, expanded ? styles.pillStacked : styles.pillInline]}>
        {attachment ? (
          <View style={styles.attachmentRow}>
            <View style={styles.thumbWrap}>
              <Image source={{ uri: attachment }} style={styles.thumb} />
              <Pressable
                accessibilityLabel="Remove attachment"
                accessibilityRole="button"
                hitSlop={6}
                onPress={onRemoveAttachment}
                style={({ pressed }) => [
                  styles.remove,
                  pressed && styles.pressed,
                ]}
              >
                <Ionicons name="close" size={12} color={palette.text} />
              </Pressable>
            </View>
          </View>
        ) : null}

        <View style={[styles.field, expanded && styles.fieldStacked]}>
          <TextInput
            accessibilityLabel={terminal ? "Command" : "Message Goble"}
            autoFocus={autoFocus}
            blurOnSubmit
            multiline={!terminal}
            onChangeText={change}
            onContentSizeChange={(event) =>
              measure(event.nativeEvent.contentSize.height)
            }
            onLayout={(event) => {
              inlineTextWidth.current = event.nativeEvent.layout.width - 84;
            }}
            onSubmitEditing={submit}
            placeholder={
              placeholder ?? (terminal ? "Run a command" : "Ask Goble")
            }
            placeholderTextColor={palette.subtle}
            returnKeyType={canSend ? "send" : "default"}
            rows={1}
            selectionColor={palette.accent}
            style={[
              styles.input,
              expanded ? styles.inputStacked : styles.inputInline,
              expanded && measuredHeight > 0
                ? { height: measuredHeight }
                : null,
            ]}
            submitBehavior={terminal ? "blurAndSubmit" : "submit"}
            testID="composer-input"
            value={value}
          />

          <View
            pointerEvents="box-none"
            style={[
              styles.controls,
              expanded ? styles.controlsStacked : styles.controlsInline,
            ]}
          >
            <View style={styles.left}>
              <Pressable
                accessibilityLabel="Add to session"
                accessibilityRole="button"
                hitSlop={6}
                onPress={() => {
                  tapFeedback();
                  onOpenAttach();
                }}
                style={({ pressed }) => [
                  styles.control,
                  pressed && styles.pressed,
                ]}
                testID="composer-add"
              >
                <Ionicons name="add" size={23} color={palette.text} />
              </Pressable>
            </View>

            <Pressable
              accessibilityLabel={
                voiceDisabled
                  ? "Dictation needs a provider that transcribes"
                  : "Dictate a message"
              }
              accessibilityRole="button"
              accessibilityState={{ disabled: voiceDisabled }}
              disabled={voiceDisabled}
              hitSlop={6}
              onPress={() => {
                tapFeedback();
                onVoice();
              }}
              style={({ pressed }) => [
                styles.voice,
                voiceDisabled && styles.voiceDisabled,
                pressed && styles.pressed,
              ]}
              testID="composer-voice"
            >
              <WaveMark />
            </Pressable>
          </View>
        </View>
      </View>
    </View>
  );
}

const WAVE_BARS = [6, 10, 14, 10, 6];

/** The composer's voice control: the five bars the dictation opens from. */
function WaveMark() {
  return (
    <View accessible={false} style={styles.wave}>
      {WAVE_BARS.map((height, index) => (
        <View
          key={`wave-${index}`}
          style={{
            backgroundColor: palette.text,
            borderRadius: 1.5,
            height,
            width: 2.5,
          }}
        />
      ))}
    </View>
  );
}

/**
 * The composer while the phone listens: a round `+` on the left, the pill that
 * carries the level of the voice, and the mute and close buttons on the right.
 * The gradient rises with the words as they arrive, so the pill reads as the
 * microphone's level until the sentence ends.
 */
function DictationBar({
  muted,
  onClose,
  onMute,
  onOpenAttach,
  text,
}: {
  muted: boolean;
  onClose: () => void;
  onMute: () => void;
  onOpenAttach: () => void;
  /** What was heard, used only to time the gradient's rise. */
  text: string;
}) {
  const level = useRef(new Animated.Value(0.4)).current;
  const sweep = useRef(new Animated.Value(0)).current;
  const [width, setWidth] = useState(0);
  const heard = useRef(0);
  const words = text.trim() ? text.trim().split(/\s+/).length : 0;

  useEffect(() => {
    const loop = Animated.loop(
      Animated.timing(sweep, {
        duration: 1800,
        toValue: 1,
        useNativeDriver: true,
      }),
    );
    loop.start();
    return () => loop.stop();
  }, [sweep]);

  useEffect(() => {
    if (words === heard.current) return;
    heard.current = words;
    Animated.sequence([
      Animated.timing(level, {
        duration: 110,
        toValue: 1,
        useNativeDriver: true,
      }),
      Animated.timing(level, {
        duration: 420,
        toValue: 0.4,
        useNativeDriver: true,
      }),
    ]).start();
  }, [level, words]);

  const scaleY = level.interpolate({
    inputRange: [0, 1],
    outputRange: [0.55, 1],
  });
  const translateY = level.interpolate({
    inputRange: [0, 1],
    outputRange: [10, 0],
  });
  const sweepX = sweep.interpolate({
    inputRange: [0, 1],
    outputRange: [0, -Math.max(width, 1) * 2],
  });

  return (
    <View style={styles.dictationRow}>
      {/* The free slot balances the close button, so the pill sits in the
          middle of the row with `+` right beside it. */}
      <View style={styles.roundSlot} />
      <RoundButton
        icon="add"
        label="Add to session"
        onPress={onOpenAttach}
      />
      <View
        onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
        style={styles.dictationPill}
      >
        <Animated.View
          style={{
            bottom: 0,
            left: 0,
            opacity: muted ? 0.18 : level,
            position: "absolute",
            right: 0,
            top: 0,
            transform: [{ scaleY }, { translateY }],
          }}
        >
          <LinearGradient
            colors={[
              withAlpha(palette.accent, 0.18),
              withAlpha(palette.accent, 0.55),
              withAlpha(palette.white, 0.9),
            ]}
            end={{ x: 0.5, y: 1 }}
            start={{ x: 0.5, y: 0 }}
            style={StyleSheet.absoluteFill}
          />
        </Animated.View>
        <Animated.View
          pointerEvents="none"
          style={{
            bottom: 0,
            left: 0,
            opacity: muted ? 0.1 : 0.55,
            position: "absolute",
            top: 0,
            transform: [{ translateX: sweepX }],
            width: Math.max(width * 2, 1),
          }}
        >
          <LinearGradient
            colors={[
              "transparent",
              withAlpha(palette.white, 0.28),
              "transparent",
            ]}
            end={{ x: 1, y: 0.5 }}
            start={{ x: 0, y: 0.5 }}
            style={StyleSheet.absoluteFill}
          />
        </Animated.View>
      </View>
      <RoundButton
        icon={muted ? "mic-off" : "mic"}
        label={muted ? "Unmute" : "Mute"}
        onPress={onMute}
        selected={muted}
      />
      <RoundButton icon="close" label="End dictation" onPress={onClose} />
    </View>
  );
}

/** One round control of the dictation bar. */
function RoundButton({
  icon,
  label,
  onPress,
  selected = false,
}: {
  icon: "add" | "close" | "mic" | "mic-off";
  label: string;
  onPress: () => void;
  selected?: boolean;
}) {
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      accessibilityState={{ selected }}
      onPress={() => {
        tapFeedback();
        onPress();
      }}
      style={({ pressed }) => [
        styles.roundButton,
        selected && styles.roundButtonSelected,
        pressed && styles.pressed,
      ]}
    >
      <Ionicons name={icon} size={20} color={palette.text} />
    </Pressable>
  );
}


const styles = StyleSheet.create({
  wrap: {
    paddingBottom: space.sm,
    paddingHorizontal: space.lg,
    paddingTop: space.sm,
  },
  pill: {
    backgroundColor: palette.surface,
    borderColor: palette.borderStrong,
    borderRadius: 26,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: "hidden",
  },
  dictationRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: 8,
  },
  dictationPill: {
    backgroundColor: palette.surface,
    borderColor: palette.borderStrong,
    borderRadius: 24,
    borderWidth: StyleSheet.hairlineWidth,
    flex: 1,
    height: 48,
    justifyContent: "center",
    overflow: "hidden",
  },
  roundSlot: {
    height: 48,
    width: 48,
  },
  roundButton: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.borderStrong,
    borderRadius: 24,
    borderWidth: StyleSheet.hairlineWidth,
    height: 48,
    justifyContent: "center",
    width: 48,
  },
  roundButtonSelected: {
    backgroundColor: palette.raised,
    borderColor: withAlpha(palette.accent, 0.55),
  },
  wave: {
    alignItems: "center",
    flexDirection: "row",
    gap: 2,
    height: 16,
    justifyContent: "center",
  },
  pillInline: {
    paddingHorizontal: 6,
    paddingVertical: 4,
  },
  pillStacked: {
    paddingBottom: 8,
    paddingHorizontal: 12,
    paddingTop: 12,
  },
  attachmentRow: {
    flexDirection: "row",
    marginBottom: 8,
    marginTop: 4,
    paddingHorizontal: 2,
  },
  thumbWrap: {
    position: "relative",
  },
  thumb: {
    backgroundColor: palette.raised,
    borderRadius: 12,
    height: 58,
    width: 58,
  },
  remove: {
    alignItems: "center",
    backgroundColor: palette.overlay,
    borderColor: palette.borderStrong,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    height: 20,
    justifyContent: "center",
    position: "absolute",
    right: 4,
    top: 4,
    width: 20,
  },
  field: {
    position: "relative",
  },
  fieldStacked: {
    marginTop: 1,
  },
  input: {
    ...typography.body,
    color: palette.text,
    fontSize: 16,
    lineHeight: 22,
    maxHeight: 148,
    outlineColor: "transparent",
    outlineStyle: "solid",
    outlineWidth: 0,
    padding: 0,
    width: "100%",
  },
  inputInline: {
    minHeight: 40,
    paddingLeft: 42,
    paddingRight: 42,
    paddingVertical: 9,
    textAlignVertical: "center",
  },
  inputStacked: {
    minHeight: 24,
    paddingHorizontal: 2,
    paddingVertical: 0,
    textAlignVertical: "top",
  },
  controls: {
    alignItems: "center",
    flexDirection: "row",
    justifyContent: "space-between",
  },
  controlsInline: {
    bottom: 0,
    left: 0,
    paddingHorizontal: 2,
    position: "absolute",
    right: 0,
    top: 0,
  },
  controlsStacked: {
    marginTop: 3,
  },
  left: {
    alignItems: "center",
    flexDirection: "row",
    gap: 1,
  },
  control: {
    alignItems: "center",
    height: 34,
    justifyContent: "center",
    width: 34,
  },
  voice: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: 17,
    borderWidth: StyleSheet.hairlineWidth,
    height: 34,
    justifyContent: "center",
    width: 34,
  },
  voiceDisabled: {
    opacity: 0.4,
  },
  pressed: {
    opacity: 0.65,
  },
});
