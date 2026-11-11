import { Ionicons } from "@expo/vector-icons";
import { useEffect, useRef, useState } from "react";
import {
  Image,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { tapFeedback } from "../lib/haptics";
import { palette, space, typography } from "../lib/theme";
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
              accessibilityLabel="Voice conversation"
              accessibilityRole="button"
              hitSlop={6}
              onPress={() => {
                tapFeedback();
                onVoice();
              }}
              style={({ pressed }) => [
                styles.voice,
                pressed && styles.pressed,
              ]}
              testID="composer-voice"
            >
              <Ionicons name="mic-outline" size={17} color={palette.text} />
            </Pressable>
          </View>
        </View>
      </View>
    </View>
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
    right: -6,
    top: -6,
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
  pressed: {
    opacity: 0.65,
  },
});
