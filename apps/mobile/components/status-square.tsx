import { StyleSheet, View } from "react-native";

import { withAlpha } from "../lib/theme";

export function StatusSquare({ color }: { color: string }) {
  return (
    <View
      style={[styles.halo, { backgroundColor: withAlpha(color, 0.16) }]}
    >
      <View style={[styles.square, { backgroundColor: color }]} />
    </View>
  );
}

const styles = StyleSheet.create({
  halo: {
    alignItems: "center",
    borderRadius: 6,
    height: 18,
    justifyContent: "center",
    width: 18,
  },
  square: {
    borderRadius: 1.5,
    height: 7,
    width: 7,
  },
});
