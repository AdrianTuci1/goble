import { Ionicons } from "@expo/vector-icons";
import { useEffect, useState } from "react";
import {
  Modal,
  Pressable,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { tapFeedback } from "../lib/haptics";
import { palette, radius, space, typography, withAlpha } from "../lib/theme";
import type { Model, ThinkingLevel } from "../lib/types";

const levels: { id: ThinkingLevel; label: string; detail: string }[] = [
  { id: "low", label: "Low", detail: "Shortest reasoning" },
  { id: "standard", label: "Standard", detail: "Balanced" },
  { id: "high", label: "High", detail: "More deliberate" },
];

export function ModelMenu({
  visible,
  models,
  model,
  thinking,
  onSelect,
  onThinking,
  onClose,
}: {
  visible: boolean;
  models: Model[];
  model: Model;
  thinking: ThinkingLevel;
  onSelect: (model: Model) => void;
  onThinking: (level: ThinkingLevel) => void;
  onClose: () => void;
}) {
  const insets = useSafeAreaInsets();
  const { width } = useWindowDimensions();
  const [page, setPage] = useState<"models" | "thinking">("models");

  useEffect(() => {
    if (visible) setPage("models");
  }, [visible]);

  const menuWidth = Math.min(320, width - 48);

  return (
    <Modal
      animationType="fade"
      onRequestClose={onClose}
      statusBarTranslucent
      transparent
      visible={visible}
    >
      <Pressable
        accessibilityLabel="Close models"
        onPress={onClose}
        style={StyleSheet.absoluteFill}
      />
      <View
        style={[
          styles.menu,
          { left: (width - menuWidth) / 2, top: insets.top + 52, width: menuWidth },
        ]}
      >
        {page === "models"
          ? models.map((option) => {
              const selected = option.id === model.id;
              return (
                <Pressable
                  accessibilityRole="button"
                  accessibilityState={{ selected }}
                  key={option.id}
                  onPress={() => {
                    tapFeedback();
                    onSelect(option);
                    onClose();
                  }}
                  style={({ pressed }) => [
                    styles.option,
                    pressed && styles.pressed,
                  ]}
                >
                  <View style={styles.check}>
                    {selected ? (
                      <Ionicons
                        name="checkmark"
                        size={19}
                        color={palette.text}
                      />
                    ) : null}
                  </View>
                  <View style={styles.optionText}>
                    <Text style={styles.optionTitle}>{option.name}</Text>
                    <Text style={styles.optionDetail}>{option.detail}</Text>
                  </View>
                </Pressable>
              );
            })
          : levels.map((option) => {
              const selected = option.id === thinking;
              return (
                <Pressable
                  accessibilityRole="button"
                  accessibilityState={{ selected }}
                  key={option.id}
                  onPress={() => {
                    tapFeedback();
                    onThinking(option.id);
                    onClose();
                  }}
                  style={({ pressed }) => [
                    styles.option,
                    pressed && styles.pressed,
                  ]}
                >
                  <View style={styles.check}>
                    {selected ? (
                      <Ionicons
                        name="checkmark"
                        size={19}
                        color={palette.text}
                      />
                    ) : null}
                  </View>
                  <View style={styles.optionText}>
                    <Text style={styles.optionTitle}>{option.label}</Text>
                    <Text style={styles.optionDetail}>{option.detail}</Text>
                  </View>
                </Pressable>
              );
            })}

        <View style={styles.divider} />

        {page === "models" ? (
          <Pressable
            accessibilityRole="button"
            onPress={() => {
              tapFeedback();
              setPage("thinking");
            }}
            style={({ pressed }) => [styles.footerRow, pressed && styles.pressed]}
          >
            <Text style={styles.footerLabel}>Thinking level</Text>
            <Ionicons
              name="chevron-forward"
              size={15}
              color={palette.subtle}
            />
          </Pressable>
        ) : (
          <Pressable
            accessibilityRole="button"
            onPress={() => {
              tapFeedback();
              setPage("models");
            }}
            style={({ pressed }) => [styles.footerRow, pressed && styles.pressed]}
          >
            <Ionicons name="chevron-back" size={15} color={palette.subtle} />
            <Text style={styles.footerLabel}>Models</Text>
          </Pressable>
        )}
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  menu: {
    backgroundColor: palette.overlay,
    borderColor: withAlpha(palette.white, 0.06),
    borderRadius: radius.xl,
    borderWidth: StyleSheet.hairlineWidth,
    paddingVertical: 6,
    position: "absolute",
    shadowColor: palette.black,
    shadowOffset: { height: 18, width: 0 },
    shadowOpacity: 0.45,
    shadowRadius: 30,
  },
  option: {
    alignItems: "center",
    flexDirection: "row",
    gap: 2,
    minHeight: 56,
    paddingHorizontal: space.md,
    paddingVertical: 7,
  },
  check: {
    alignItems: "center",
    height: 24,
    justifyContent: "center",
    width: 28,
  },
  optionText: {
    flex: 1,
    gap: 1,
  },
  optionTitle: {
    color: palette.text,
    fontSize: 17,
    lineHeight: 22,
  },
  optionDetail: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  divider: {
    backgroundColor: withAlpha(palette.white, 0.08),
    height: StyleSheet.hairlineWidth,
    marginHorizontal: space.md,
    marginVertical: 4,
  },
  footerRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
    minHeight: 48,
    paddingHorizontal: space.md,
    paddingVertical: 6,
  },
  footerLabel: {
    color: palette.text,
    flex: 1,
    fontSize: 16,
  },
  pressed: {
    backgroundColor: withAlpha(palette.white, 0.05),
  },
});
