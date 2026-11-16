import { configurePlatform } from "@goble/common";

configurePlatform({
  storage: {
    async get(key) {
      try {
        return globalThis.localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    async set(key, value) {
      try {
        globalThis.localStorage.setItem(key, value);
      } catch {
        // A browser without storage keeps the value in memory only.
      }
    },
    async remove(key) {
      try {
        globalThis.localStorage.removeItem(key);
      } catch {
        // Ignore missing browser storage.
      }
    },
  },
  haptics: {
    tap() {},
    success() {},
    error() {},
  },
});
