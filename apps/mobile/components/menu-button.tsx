import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet } from "react-native";

import { tapFeedback } from "../lib/haptics";
import { palette, radius } from "../lib/theme";

export function MenuButton({ onPress }: { onPress: () => void }) {
  return (
    <Pressable
      accessibilityLabel="Open sessions"
      accessibilityRole="button"
      hitSlop={8}
      testID="open-sessions"
      onPress={() => {
        tapFeedback();
        onPress();
      }}
      style={({ pressed }) => [styles.button, pressed && styles.pressed]}
    >
      <Ionicons name="menu" size={20} color={palette.text} />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  button: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  pressed: {
    opacity: 0.65,
  },
});
