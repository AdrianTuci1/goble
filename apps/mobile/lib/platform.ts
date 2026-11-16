import * as SecureStore from "expo-secure-store";
import { Platform } from "react-native";

import { configurePlatform } from "@goble/common";

import { errorFeedback, successFeedback, tapFeedback } from "./haptics";

configurePlatform({
  haptics: {
    tap: tapFeedback,
    success: successFeedback,
    error: errorFeedback,
  },
});

if (Platform.OS !== "web") {
  configurePlatform({
    storage: {
      async get(key) {
        try {
          return await SecureStore.getItemAsync(key);
        } catch {
          return null;
        }
      },
      async set(key, value) {
        try {
          await SecureStore.setItemAsync(key, value);
        } catch {
          // Some hosts have no keychain; the value stays in the app only.
        }
      },
      async remove(key) {
        try {
          await SecureStore.deleteItemAsync(key);
        } catch {
          // Ignore missing native storage.
        }
      },
    },
  });
}
