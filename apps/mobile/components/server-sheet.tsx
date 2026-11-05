import { Ionicons } from "@expo/vector-icons";
import { useEffect, useState } from "react";
import { Pressable, StyleSheet, Text, TextInput, View } from "react-native";

import { normalizeServerUrl, type KnownServer } from "../lib/api";
import { hostLabel } from "../lib/format";
import { tapFeedback } from "../lib/haptics";
import { palette, radius, space, typography } from "../lib/theme";
import { Sheet } from "./sheet";

export function ServerSheet({
  visible,
  servers,
  current,
  onSelect,
  onClose,
}: {
  visible: boolean;
  servers: KnownServer[];
  current: string;
  onSelect: (server: KnownServer) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState("");
  const [draftName, setDraftName] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (visible) {
      setDraft("");
      setDraftName("");
      setError(null);
    }
  }, [visible]);

  const normalized = normalizeServerUrl(draft);

  function add() {
    if (!normalized.ok) {
      setError(normalized.error);
      return;
    }
    tapFeedback();
    onSelect({
      url: normalized.url,
      label: draftName.trim() || hostLabel(normalized.url),
      usedAt: new Date().toISOString(),
    });
    onClose();
  }

  return (
    <Sheet onClose={onClose} visible={visible}>
      <Text style={styles.eyebrow}>GATEWAY</Text>
      <Text style={styles.title}>Choose a server</Text>

      <View style={styles.list}>
        {servers.map((server) => {
          const selected = server.url === current;
          return (
            <Pressable
              accessibilityLabel={`Server ${server.label}`}
              accessibilityRole="button"
              accessibilityState={{ selected }}
              key={server.url}
              onPress={() => {
                tapFeedback();
                onSelect(server);
                onClose();
              }}
              style={({ pressed }) => [
                styles.option,
                selected && styles.optionActive,
                pressed && styles.pressed,
              ]}
            >
              <View style={styles.icon}>
                <Ionicons
                  name="server-outline"
                  size={17}
                  color={selected ? palette.accent : palette.muted}
                />
              </View>
              <View style={styles.text}>
                <Text style={styles.name}>{server.label}</Text>
                <Text numberOfLines={1} style={styles.url}>
                  {server.url}
                </Text>
              </View>
              {selected ? (
                <Ionicons
                  name="checkmark-circle"
                  size={18}
                  color={palette.accent}
                />
              ) : (
                <View style={styles.radio} />
              )}
            </Pressable>
          );
        })}
      </View>

      <Text style={styles.addLabel}>ADD A SERVER</Text>
      <View style={styles.inputWrap}>
        <Ionicons name="pricetag-outline" size={16} color={palette.subtle} />
        <TextInput
          accessibilityLabel="Server name"
          autoCapitalize="words"
          autoCorrect={false}
          onChangeText={setDraftName}
          onSubmitEditing={add}
          placeholder="Friendly name, e.g. Home gateway"
          placeholderTextColor={palette.subtle}
          returnKeyType="next"
          selectionColor={palette.accent}
          style={styles.input}
          value={draftName}
        />
      </View>
      <View style={styles.inputWrap}>
        <Ionicons name="link-outline" size={16} color={palette.subtle} />
        <TextInput
          accessibilityLabel="New server URL"
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
          onChangeText={(value) => {
            setDraft(value);
            setError(null);
          }}
          onSubmitEditing={add}
          placeholder="https://gateway.example.com"
          placeholderTextColor={palette.subtle}
          returnKeyType="go"
          selectionColor={palette.accent}
          style={styles.input}
          value={draft}
        />
      </View>
      {error ? <Text style={styles.error}>{error}</Text> : null}

      <Pressable
        accessibilityRole="button"
        accessibilityState={{ disabled: !normalized.ok }}
        disabled={!normalized.ok}
        onPress={add}
        style={({ pressed }) => [
          styles.addButton,
          !normalized.ok && styles.addButtonDisabled,
          pressed && styles.pressed,
        ]}
      >
        <Text style={styles.addButtonLabel}>Use this server</Text>
      </Pressable>

      <Text style={styles.footer}>
        Servers you use are kept on this device.
      </Text>
    </Sheet>
  );
}

const styles = StyleSheet.create({
  eyebrow: {
    ...typography.eyebrow,
    color: palette.muted,
    marginBottom: space.xs,
  },
  title: {
    ...typography.title,
    color: palette.text,
    marginBottom: space.lg,
  },
  list: {
    gap: space.sm,
  },
  option: {
    alignItems: "center",
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.md,
    padding: space.md,
  },
  optionActive: {
    backgroundColor: palette.raised,
    borderColor: palette.borderStrong,
  },
  icon: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderRadius: radius.sm,
    height: 36,
    justifyContent: "center",
    width: 36,
  },
  text: {
    flex: 1,
    gap: 2,
  },
  name: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  url: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
  },
  radio: {
    borderColor: palette.borderStrong,
    borderRadius: 9,
    borderWidth: 1,
    height: 18,
    width: 18,
  },
  addLabel: {
    ...typography.eyebrow,
    color: palette.muted,
    marginTop: space.lg,
  },
  inputWrap: {
    alignItems: "center",
    backgroundColor: palette.bg,
    borderColor: palette.borderStrong,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.sm,
    marginTop: space.sm,
    paddingHorizontal: space.md,
  },
  input: {
    ...typography.body,
    color: palette.text,
    flex: 1,
    fontSize: 14,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingVertical: 12,
  },
  error: {
    ...typography.caption,
    color: palette.error,
    marginTop: space.sm,
  },
  addButton: {
    alignItems: "center",
    backgroundColor: palette.primary,
    borderRadius: radius.md,
    justifyContent: "center",
    marginTop: space.md,
    paddingVertical: 12,
  },
  addButtonDisabled: {
    opacity: 0.35,
  },
  addButtonLabel: {
    color: palette.black,
    fontSize: 14,
    fontWeight: "600",
  },
  footer: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
    paddingTop: space.lg,
  },
  pressed: {
    opacity: 0.7,
  },
});
