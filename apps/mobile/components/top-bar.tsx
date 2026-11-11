import { Ionicons } from "@expo/vector-icons";
import type { ReactNode } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";

import { tapFeedback } from "../lib/haptics";
import { palette, space, typography } from "../lib/theme";
import type { Model } from "../lib/types";
import { Avatar } from "./avatar";
import { MenuButton } from "./menu-button";

export function TopBar({
  model,
  onMenu,
  onModel,
  onAvatar,
  right,
}: {
  model: Model;
  onMenu: () => void;
  onModel: () => void;
  onAvatar?: () => void;
  right?: ReactNode;
}) {
  return (
    <View style={styles.bar}>
      <View style={styles.side}>
        <MenuButton onPress={onMenu} />
      </View>

      <Pressable
        accessibilityLabel={`Model: ${model.name}`}
        accessibilityRole="button"
        onPress={() => {
          tapFeedback();
          onModel();
        }}
        style={({ pressed }) => [styles.center, pressed && styles.pressed]}
      >
        <Text style={styles.model} numberOfLines={1}>
          {model.name}
        </Text>
        <Ionicons name="chevron-down" size={13} color={palette.subtle} />
      </Pressable>

      <View style={[styles.side, styles.sideEnd]}>
        {right ?? (
          <Pressable
            accessibilityLabel="Account"
            accessibilityRole="button"
            hitSlop={8}
            onPress={onAvatar}
            style={({ pressed }) => pressed && styles.pressed}
          >
            <Avatar />
          </Pressable>
        )}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  bar: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
    minHeight: 52,
    paddingHorizontal: space.lg,
    paddingVertical: space.sm,
  },
  side: {
    flex: 1,
    flexDirection: "row",
  },
  sideEnd: {
    justifyContent: "flex-end",
  },
  center: {
    alignItems: "center",
    flexDirection: "row",
    gap: 5,
  },
  model: {
    ...typography.callout,
    color: palette.text,
    fontSize: 16,
    fontWeight: "500",
    letterSpacing: -0.2,
  },
  pressed: {
    opacity: 0.62,
  },
});
