import { StyleSheet, Text, View } from "react-native";

import { mono, palette, radius } from "../lib/theme";

export function BrandMark({ size = 30 }: { size?: number }) {
  return (
    <View
      style={[
        styles.mark,
        {
          width: size,
          height: size,
          borderRadius: radius.sm,
        },
      ]}
    >
      <Text style={[styles.glyph, { fontSize: Math.round(size * 0.42) }]}>
        {">_"}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  mark: {
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: palette.raised,
    borderColor: palette.borderStrong,
    borderWidth: StyleSheet.hairlineWidth,
  },
  glyph: {
    color: palette.accent,
    fontFamily: mono,
    fontWeight: "700",
    letterSpacing: -1.5,
  },
});
