import "react-native-gesture-handler";

import { Stack } from "expo-router";
import { useEffect } from "react";
import { StatusBar } from "expo-status-bar";
import { StyleSheet } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { AuthGate } from "../components/auth-gate";
import { syncPushRegistration } from "../lib/notifications";
import { palette } from "../lib/theme";

export default function RootLayout() {
  useEffect(() => {
    void syncPushRegistration();
  }, []);

  return (
    <GestureHandlerRootView style={styles.root}>
      <SafeAreaProvider>
        <StatusBar style="light" />
        <AuthGate>
          <Stack
            screenOptions={{
              headerShown: false,
              contentStyle: { backgroundColor: palette.bg },
              animation: "slide_from_right",
            }}
          >
            <Stack.Screen name="(drawer)" />
            <Stack.Screen name="sign-in" options={{ animation: "fade" }} />
            <Stack.Screen name="+not-found" />
          </Stack>
        </AuthGate>
      </SafeAreaProvider>
    </GestureHandlerRootView>
  );
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.bg,
    flex: 1,
  },
});
