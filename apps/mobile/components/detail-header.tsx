import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet, Text, View } from "react-native";
import type { ReactNode } from "react";

import { tapFeedback } from "../lib/haptics";
import { palette, radius, space, typography } from "../lib/theme";

export function DetailHeader({
  title,
  subtitle,
  onBack,
  right,
}: {
  title: string;
  subtitle?: string;
  onBack: () => void;
  right?: ReactNode;
}) {
  return (
    <View style={styles.header}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Back"
        hitSlop={8}
        onPress={() => {
          tapFeedback();
          onBack();
        }}
        style={({ pressed }) => [styles.back, pressed && styles.pressed]}
      >
        <Ionicons name="chevron-back" size={18} color={palette.text} />
      </Pressable>
      <View style={styles.text}>
        <Text style={styles.title} numberOfLines={1}>
          {title}
        </Text>
        {subtitle ? (
          <Text style={styles.subtitle} numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {right ?? <View style={styles.placeholder} />}
    </View>
  );
}

const styles = StyleSheet.create({
  header: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    paddingBottom: space.md,
    paddingTop: space.sm,
  },
  back: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    height: 34,
    justifyContent: "center",
    width: 34,
  },
  pressed: {
    opacity: 0.65,
  },
  text: {
    flex: 1,
    gap: 1,
  },
  title: {
    ...typography.title,
    color: palette.text,
    fontSize: 17,
    lineHeight: 22,
  },
  subtitle: {
    ...typography.caption,
    color: palette.muted,
  },
  placeholder: {
    width: 34,
  },
});
