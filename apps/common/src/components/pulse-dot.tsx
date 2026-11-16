import { useEffect, useRef } from "react";
import { Animated, StyleSheet, View } from "react-native";

import { palette } from "../lib/theme";

export function PulseDot({
  color = palette.success,
  size = 7,
}: {
  color?: string;
  size?: number;
}) {
  const value = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    const loop = Animated.loop(
      Animated.sequence([
        Animated.timing(value, {
          toValue: 1,
          duration: 900,
          useNativeDriver: true,
        }),
        Animated.timing(value, {
          toValue: 0,
          duration: 900,
          useNativeDriver: true,
        }),
      ]),
    );
    loop.start();
    return () => loop.stop();
  }, [value]);

  const scale = value.interpolate({
    inputRange: [0, 1],
    outputRange: [1, 1.9],
  });
  const opacity = value.interpolate({
    inputRange: [0, 1],
    outputRange: [0.5, 0],
  });

  return (
    <View style={[styles.wrap, { width: size, height: size }]}>
      <Animated.View
        style={[
          styles.halo,
          {
            backgroundColor: color,
            borderRadius: size,
            opacity,
            transform: [{ scale }],
          },
        ]}
      />
      <View
        style={{
          backgroundColor: color,
          borderRadius: size,
          height: size,
          width: size,
        }}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: {
    alignItems: "center",
    justifyContent: "center",
  },
  halo: {
    bottom: 0,
    left: 0,
    position: "absolute",
    right: 0,
    top: 0,
  },
});
