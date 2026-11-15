import { useRouter } from "expo-router";
import {
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
import { useEffect, useState } from "react";

import { BrandMark } from "../components/brand-mark";
import { ServerSheet } from "../components/server-sheet";
import { SettingsButton } from "../components/settings/settings-button";
import { SettingsRow } from "../components/settings/settings-row";
import {
  SettingsSection,
  SettingsSectionLabel,
} from "../components/settings/settings-section";
import {
  LOCAL_GATEWAY,
  loadKnownServers,
  normalizeServerUrl,
  probeServer,
  rememberServer,
  saveAuthMode,
  signOut,
  type KnownServer,
} from "../lib/api";
import { connection } from "../lib/remote/connection";
import { hostLabel } from "../lib/format";
import { successFeedback, tapFeedback } from "../lib/haptics";
import { palette, radius, space } from "../lib/theme";

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
  const [pin, setPin] = useState("");
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
  const inactive = pending || !normalized.ok;

  async function submit() {
    if (pending) return;
    if (!normalized.ok) {
      setError(normalized.error);
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
      const paired = await connection.pair(normalized.url, pin);
      if (!paired.ok) {
        setError(paired.message);
        return;
      }
      await rememberServer({
        url: normalized.url,
        label: serverName.trim() || hostLabel(normalized.url),
        usedAt: new Date().toISOString(),
      });
      successFeedback();
      router.replace("/");
    } finally {
      setPending(false);
    }
  }

  async function useDemo() {
    tapFeedback();
    await connection.disconnect();
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
              showsVerticalScrollIndicator={false}
            >
              <View style={styles.brand}>
                <BrandMark size={36} />
                <Text accessibilityRole="header" style={styles.title}>
                  Sign in to Goble
                </Text>
                <Text style={styles.subtitle}>
                  Connect this phone to the gateway that runs your agents.
                </Text>
              </View>

              <SettingsSection title="Gateway">
                <SettingsRow
                  description={hostLabel(url)}
                  icon="server-outline"
                  label={serverName}
                  onPress={() => {
                    tapFeedback();
                    setPicker(true);
                  }}
                />
              </SettingsSection>

              <View style={styles.form}>
                <SettingsSectionLabel>Pairing</SettingsSectionLabel>
                <TextInput
                  accessibilityLabel="Gateway PIN"
                  autoCapitalize="characters"
                  autoCorrect={false}
                  onChangeText={(value) => {
                    setPin(value.toUpperCase());
                    setError(null);
                  }}
                  onSubmitEditing={() => void submit()}
                  placeholder="Gateway PIN"
                  placeholderTextColor={palette.subtle}
                  returnKeyType="go"
                  selectionColor={palette.accent}
                  style={styles.input}
                  value={pin}
                />
                <Text style={styles.hint}>
                  The PIN is in the gateway panel on the machine that runs the
                  agents. A gateway without a PIN pairs with the field empty.
                </Text>
                {error ? (
                  <Text accessibilityRole="alert" style={styles.error}>
                    {error}
                  </Text>
                ) : null}
              </View>

              <SettingsButton
                disabled={inactive}
                label="Pair with the gateway"
                loading={pending}
                onPress={() => void submit()}
              />

              <Pressable
                accessibilityRole="button"
                hitSlop={8}
                onPress={() => void useDemo()}
                style={({ pressed }) => [styles.quiet, pressed && styles.pressed]}
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
    gap: space.xl,
    justifyContent: "center",
    paddingBottom: space.xxl,
    paddingHorizontal: 20,
    paddingTop: space.xxl,
  },
  brand: {
    alignItems: "center",
    gap: space.sm,
    marginBottom: space.xs,
  },
  title: {
    color: palette.text,
    fontSize: 28,
    fontWeight: "600",
    letterSpacing: -0.6,
    lineHeight: 34,
    marginTop: space.xs,
    textAlign: "center",
  },
  subtitle: {
    color: palette.muted,
    fontSize: 14,
    lineHeight: 19,
    maxWidth: 300,
    textAlign: "center",
  },
  form: {
    gap: space.sm,
  },
  input: {
    backgroundColor: palette.surface,
    borderRadius: radius.md + 2,
    color: palette.text,
    fontSize: 16,
    minHeight: 54,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingHorizontal: 14,
    paddingVertical: 14,
  },
  error: {
    color: palette.error,
    fontSize: 13,
    lineHeight: 18,
    paddingHorizontal: space.sm,
  },
  hint: {
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 17,
    paddingHorizontal: space.sm,
  },
  quiet: {
    alignSelf: "center",
    paddingVertical: space.sm,
  },
  quietLabel: {
    color: palette.muted,
    fontSize: 14,
    lineHeight: 19,
  },
  pressed: {
    opacity: 0.7,
  },
});
