import { Ionicons } from "@expo/vector-icons";
import type { ReactNode } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";

import { tapFeedback } from "../lib/haptics";
import { palette, space, typography } from "../lib/theme";
import type { Model } from "../lib/types";
import { MenuButton } from "./menu-button";

export function TopBar({
  model,
  onMenu,
  onModel,
  title,
  right,
}: {
  model: Model;
  onMenu: () => void;
  onModel: () => void;
  /**
   * What the bar names instead of the model. A terminal has no model to pick,
   * so it passes its own word and the chevron goes away.
   */
  title?: string;
  /** The actions on the right of the bar, owned by the screen that draws it. */
  right?: ReactNode;
}) {
  const label = title ?? model.name;

  return (
    <View style={styles.bar}>
      <View style={styles.side}>
        <MenuButton onPress={onMenu} />
      </View>

      {title ? (
        <Text numberOfLines={1} style={styles.title}>
          {label}
        </Text>
      ) : (
        <Pressable
          accessibilityLabel={`Model: ${label}`}
          accessibilityRole="button"
          onPress={() => {
            tapFeedback();
            onModel();
          }}
          style={({ pressed }) => [styles.center, pressed && styles.pressed]}
        >
          <Text style={styles.model} numberOfLines={1}>
            {label}
          </Text>
          <Ionicons name="chevron-down" size={13} color={palette.subtle} />
        </Pressable>
      )}

      <View style={[styles.side, styles.sideEnd]}>{right}</View>
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
  title: {
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
