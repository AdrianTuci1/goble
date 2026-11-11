import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet, Text, View } from "react-native";

import { tapFeedback } from "../lib/haptics";
import { palette, radius, space, typography } from "../lib/theme";
import type { ComponentProps } from "react";

type IconName = ComponentProps<typeof Ionicons>["name"];

export function QuickAction({
  label,
  icon,
  onPress,
  primary = false,
}: {
  label: string;
  icon: IconName;
  onPress: () => void;
  primary?: boolean;
}) {
  return (
    <Pressable
      accessibilityRole="button"
      onPress={() => {
        tapFeedback();
        onPress();
      }}
      style={({ pressed }) => [
        styles.button,
        primary ? styles.primary : styles.secondary,
        pressed && styles.pressed,
      ]}
    >
      <View style={[styles.icon, primary && styles.primaryIcon]}>
        <Ionicons
          name={icon}
          size={15}
          color={primary ? palette.black : palette.text}
        />
      </View>
      <Text style={[styles.label, primary && styles.primaryLabel]}>{label}</Text>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  button: {
    alignItems: "center",
    borderRadius: radius.md,
    flex: 1,
    flexDirection: "row",
    gap: space.sm,
    justifyContent: "center",
    paddingHorizontal: space.md,
    paddingVertical: 12,
  },
  primary: {
    backgroundColor: palette.text,
  },
  secondary: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderWidth: StyleSheet.hairlineWidth,
  },
  pressed: {
    opacity: 0.72,
  },
  icon: {
    alignItems: "center",
    justifyContent: "center",
  },
  primaryIcon: {
    opacity: 0.82,
  },
  label: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
  },
  primaryLabel: {
    color: palette.black,
  },
});
