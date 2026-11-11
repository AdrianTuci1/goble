import { LinearGradient } from "expo-linear-gradient";
import { StyleSheet, Text, View } from "react-native";

import { palette, typography } from "../lib/theme";

export function Avatar({ size = 34 }: { size?: number }) {
  return (
    <LinearGradient
      colors={["#19aad8", "#8b5cf6", "#10b981"]}
      end={{ x: 1, y: 1 }}
      start={{ x: 0, y: 0 }}
      style={{ borderRadius: size, height: size, padding: 1.5, width: size }}
    >
      <View
        style={[
          styles.inner,
          { borderRadius: size - 1.5, height: size - 3, width: size - 3 },
        ]}
      >
        <Text style={[styles.initial, { fontSize: size * 0.36 }]}>A</Text>
      </View>
    </LinearGradient>
  );
}

const styles = StyleSheet.create({
  inner: {
    alignItems: "center",
    backgroundColor: palette.surface,
    flex: 1,
    justifyContent: "center",
  },
  initial: {
    ...typography.callout,
    color: palette.text,
    fontWeight: "600",
  },
});
