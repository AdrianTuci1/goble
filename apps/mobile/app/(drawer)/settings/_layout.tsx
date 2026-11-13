import { Stack } from "expo-router";

import { palette } from "../../../lib/theme";

export default function SettingsLayout() {
  return (
    <Stack
      screenOptions={{
        animation: "slide_from_right",
        contentStyle: { backgroundColor: palette.bg },
        headerShown: false,
      }}
    />
  );
}
