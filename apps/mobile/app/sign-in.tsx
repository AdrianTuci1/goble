import { Ionicons } from "@expo/vector-icons";
import { useRouter } from "expo-router";
import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  Keyboard,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  TouchableWithoutFeedback,
  View,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { ServerSheet } from "../components/server-sheet";
import {
  LOCAL_GATEWAY,
  loadKnownServers,
  normalizeServerUrl,
  probeServer,
  rememberServer,
  saveAuthMode,
  saveServerUrl,
  signOut,
  type KnownServer,
} from "../lib/api";
import { hostLabel } from "../lib/format";
import { successFeedback, tapFeedback } from "../lib/haptics";
import { palette, space, typography } from "../lib/theme";

const LOCAL_SERVER: KnownServer = {
  url: LOCAL_GATEWAY,
  label: "This Mac",
  usedAt: "",
};

export default function SignInScreen() {
  const router = useRouter();
  const [servers, setServers] = useState<KnownServer[]>([LOCAL_SERVER]);
  const [url, setUrl] = useState(LOCAL_GATEWAY);
  const [serverName, setServerName] = useState(LOCAL_SERVER.label);
  const [identity, setIdentity] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [picker, setPicker] = useState(false);

  useEffect(() => {
    void loadKnownServers().then((known) => {
      const list = known.some((server) => server.url === LOCAL_GATEWAY)
        ? known
        : [LOCAL_SERVER, ...known];
      setServers(list);
      const first = known[0] ?? LOCAL_SERVER;
      setUrl(first.url);
      setServerName(first.label);
    });
  }, []);

  const normalized = normalizeServerUrl(url);
  const inactive = pending || !normalized.ok || !identity.trim() || !password;

  async function submit() {
    if (pending) return;
    if (!normalized.ok) {
      setError(normalized.error);
      return;
    }
    if (!identity.trim()) {
      setError("Enter your email or username");
      return;
    }
    if (password.length < 6) {
      setError("Password needs at least 6 characters");
      return;
    }
    setPending(true);
    setError(null);
    try {
      const result = await probeServer(normalized.url);
      if (!result.ok) {
        setError(result.message);
        return;
      }
      await saveServerUrl(normalized.url);
      await rememberServer({
        url: normalized.url,
        label: serverName.trim() || hostLabel(normalized.url),
        usedAt: new Date().toISOString(),
      });
      await saveAuthMode("server");
      successFeedback();
      router.replace("/");
    } finally {
      setPending(false);
    }
  }

  async function useDemo() {
    tapFeedback();
    await signOut();
    await saveAuthMode("demo");
    router.replace("/");
  }

  return (
    <SafeAreaView style={styles.safe}>
      <KeyboardAvoidingView
        behavior={Platform.OS === "ios" ? "padding" : undefined}
        style={styles.flex}
      >
        <TouchableWithoutFeedback accessible={false} onPress={Keyboard.dismiss}>
          <View style={styles.flex}>
            <ScrollView
              contentContainerStyle={styles.scroll}
              keyboardDismissMode={
                Platform.OS === "ios" ? "interactive" : "on-drag"
              }
              keyboardShouldPersistTaps="handled"
            >
              <Text accessibilityRole="header" style={styles.title}>
                Sign in to Goble
              </Text>

              <Pressable
                accessibilityLabel={`Server ${serverName}`}
                accessibilityRole="button"
                onPress={() => {
                  tapFeedback();
                  setPicker(true);
                }}
                style={({ pressed }) => [
                  styles.select,
                  pressed && styles.pressed,
                ]}
              >
                <Ionicons
                  name="server-outline"
                  size={17}
                  color={palette.muted}
                />
                <View style={styles.selectText}>
                  <Text numberOfLines={1} style={styles.selectLabel}>
                    {serverName}
                  </Text>
                  <Text numberOfLines={1} style={styles.selectHost}>
                    {hostLabel(url)}
                  </Text>
                </View>
                <Ionicons
                  name="chevron-down"
                  size={15}
                  color={palette.subtle}
                />
              </Pressable>

              <TextInput
                accessibilityLabel="Email or username"
                autoCapitalize="none"
                autoComplete="username"
                autoCorrect={false}
                keyboardType="email-address"
                onChangeText={(value) => {
                  setIdentity(value);
                  setError(null);
                }}
                onSubmitEditing={() => void submit()}
                placeholder="Email or username"
                placeholderTextColor={palette.subtle}
                returnKeyType="next"
                selectionColor={palette.accent}
                style={styles.input}
                textContentType="username"
                value={identity}
              />

              <TextInput
                accessibilityLabel="Password"
                autoComplete="current-password"
                onChangeText={(value) => {
                  setPassword(value);
                  setError(null);
                }}
                onSubmitEditing={() => void submit()}
                placeholder="Password"
                placeholderTextColor={palette.subtle}
                returnKeyType="go"
                secureTextEntry
                selectionColor={palette.accent}
                style={styles.input}
                textContentType="password"
                value={password}
              />

              {error ? (
                <Text accessibilityRole="alert" style={styles.error}>
                  {error}
                </Text>
              ) : null}

              <Pressable
                accessibilityRole="button"
                accessibilityState={{ disabled: inactive }}
                disabled={inactive}
                onPress={() => void submit()}
                style={({ pressed }) => [
                  styles.primary,
                  inactive && styles.primaryInactive,
                  pressed && styles.pressed,
                ]}
              >
                {pending ? (
                  <ActivityIndicator color={palette.black} size="small" />
                ) : (
                  <Text style={styles.primaryLabel}>
                    {pending ? "Working…" : "Sign in"}
                  </Text>
                )}
              </Pressable>

              <Pressable
                accessibilityRole="button"
                hitSlop={8}
                onPress={() => void useDemo()}
                style={({ pressed }) => [
                  styles.quiet,
                  pressed && styles.pressed,
                ]}
              >
                <Text style={styles.quietLabel}>
                  Continue with the demo workspace
                </Text>
              </Pressable>
            </ScrollView>
          </View>
        </TouchableWithoutFeedback>
      </KeyboardAvoidingView>

      <ServerSheet
        current={url}
        onClose={() => setPicker(false)}
        onSelect={(server) => {
          setUrl(server.url);
          setServerName(server.label);
          setError(null);
          setServers((list) =>
            list.some((item) => item.url === server.url)
              ? list
              : [server, ...list],
          );
        }}
        servers={servers}
        visible={picker}
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safe: {
    backgroundColor: palette.bg,
    flex: 1,
  },
  flex: {
    flex: 1,
  },
  scroll: {
    flexGrow: 1,
    justifyContent: "center",
    paddingHorizontal: space.xl,
    paddingVertical: space.xl,
  },
  title: {
    color: palette.text,
    fontSize: 32,
    fontWeight: "500",
    lineHeight: 38,
    textAlign: "center",
  },
  select: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderRadius: 13,
    flexDirection: "row",
    gap: space.sm,
    marginTop: 28,
    padding: 16,
  },
  selectText: {
    flex: 1,
    gap: 2,
  },
  selectLabel: {
    color: palette.text,
    fontSize: 16,
  },
  selectHost: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 11,
  },
  input: {
    backgroundColor: palette.surface,
    borderRadius: 13,
    color: palette.text,
    fontSize: 16,
    marginTop: 12,
    outlineColor: "transparent",
    outlineStyle: "solid",
    padding: 16,
  },
  error: {
    color: palette.error,
    marginTop: 12,
  },
  primary: {
    alignItems: "center",
    backgroundColor: palette.primary,
    borderRadius: 12,
    justifyContent: "center",
    marginTop: 16,
    minHeight: 48,
    paddingHorizontal: 16,
    paddingVertical: 12,
  },
  primaryInactive: {
    opacity: 0.45,
  },
  primaryLabel: {
    color: palette.black,
    fontSize: 16,
    fontWeight: "600",
  },
  quiet: {
    alignSelf: "center",
    marginTop: 16,
    paddingVertical: 8,
  },
  quietLabel: {
    color: palette.muted,
    fontSize: 15,
  },
  pressed: {
    opacity: 0.7,
  },
});
