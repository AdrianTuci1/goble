import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet, Text, View } from "react-native";

import type { AttachmentSource } from "../lib/attachments";
import { tapFeedback } from "../lib/haptics";
import { palette, radius, space, typography, withAlpha } from "../lib/theme";
import type { SessionMode } from "../lib/types";
import { Sheet } from "./sheet";

const modes = [
  {
    mode: "agent" as const,
    icon: "sparkles" as const,
    title: "Agent",
    description: "Plan, edit and run tools.",
  },
  {
    mode: "terminal" as const,
    icon: "terminal-outline" as const,
    title: "Terminal",
    description: "Run a shell command.",
  },
];

export function ComposerSheet({
  visible,
  mode,
  onSelectMode,
  onPick,
  onClose,
}: {
  visible: boolean;
  mode: SessionMode;
  onSelectMode: (mode: SessionMode) => void;
  onPick: (source: AttachmentSource) => void;
  onClose: () => void;
}) {
  return (
    <Sheet onClose={onClose} visible={visible}>
      <Text style={styles.eyebrow}>ADD TO SESSION</Text>
      <Text style={styles.title}>Photos, camera or mode</Text>

      <View style={styles.mediaRow}>
        <Pressable
          accessibilityLabel="Add a photo"
          accessibilityRole="button"
          onPress={() => {
            tapFeedback();
            onPick("library");
          }}
          style={({ pressed }) => [styles.media, pressed && styles.pressed]}
        >
          <View style={styles.mediaIcon}>
            <Ionicons name="images-outline" size={24} color={palette.text} />
          </View>
          <Text style={styles.mediaLabel}>Photos</Text>
          <Text style={styles.mediaBody}>From your library</Text>
        </Pressable>

        <Pressable
          accessibilityLabel="Take a photo"
          accessibilityRole="button"
          onPress={() => {
            tapFeedback();
            onPick("camera");
          }}
          style={({ pressed }) => [styles.media, pressed && styles.pressed]}
        >
          <View style={styles.mediaIcon}>
            <Ionicons name="camera-outline" size={24} color={palette.text} />
          </View>
          <Text style={styles.mediaLabel}>Camera</Text>
          <Text style={styles.mediaBody}>Take a photo</Text>
        </Pressable>
      </View>

      <Text style={styles.sectionLabel}>MODE</Text>
      <View style={styles.list}>
        {modes.map((option) => {
          const active = option.mode === mode;
          return (
            <Pressable
              accessibilityRole="button"
              accessibilityState={{ selected: active }}
              key={option.mode}
              onPress={() => {
                tapFeedback();
                onSelectMode(option.mode);
                onClose();
              }}
              style={({ pressed }) => [
                styles.option,
                active && styles.optionActive,
                pressed && styles.pressed,
              ]}
            >
              <View
                style={[styles.optionIcon, active && styles.optionIconActive]}
              >
                <Ionicons
                  name={option.icon}
                  size={17}
                  color={active ? palette.accent : palette.muted}
                />
              </View>
              <View style={styles.optionText}>
                <Text style={styles.optionTitle}>{option.title}</Text>
                <Text style={styles.optionBody}>{option.description}</Text>
              </View>
              {active ? (
                <Ionicons
                  name="checkmark-circle"
                  size={19}
                  color={palette.accent}
                />
              ) : (
                <View style={styles.radio} />
              )}
            </Pressable>
          );
        })}
      </View>
    </Sheet>
  );
}

const styles = StyleSheet.create({
  eyebrow: {
    ...typography.eyebrow,
    color: palette.muted,
    marginBottom: space.xs,
  },
  title: {
    ...typography.title,
    color: palette.text,
    marginBottom: space.lg,
  },
  mediaRow: {
    flexDirection: "row",
    gap: space.md,
  },
  media: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flex: 1,
    gap: 4,
    paddingHorizontal: space.md,
    paddingVertical: space.lg,
  },
  mediaIcon: {
    alignItems: "center",
    height: 34,
    justifyContent: "center",
    marginBottom: 2,
  },
  mediaLabel: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  mediaBody: {
    ...typography.caption,
    color: palette.subtle,
    fontSize: 11,
  },
  sectionLabel: {
    ...typography.eyebrow,
    color: palette.muted,
    marginTop: space.lg,
  },
  list: {
    gap: space.sm,
    marginTop: space.sm,
  },
  option: {
    alignItems: "center",
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.md,
    padding: space.md,
  },
  optionActive: {
    backgroundColor: withAlpha(palette.accent, 0.06),
    borderColor: withAlpha(palette.accent, 0.35),
  },
  optionIcon: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderRadius: radius.md,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  optionIconActive: {
    backgroundColor: palette.accentSoft,
  },
  optionText: {
    flex: 1,
    gap: 1,
  },
  optionTitle: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  optionBody: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
  },
  radio: {
    borderColor: palette.borderStrong,
    borderRadius: 9,
    borderWidth: 1,
    height: 18,
    width: 18,
  },
  pressed: {
    opacity: 0.7,
  },
});
