import { useEffect, useState } from "react";
import { Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { tapFeedback } from "../lib/haptics";
import { palette, radius, space, typography } from "../lib/theme";
import type { Conversation } from "../lib/types";
import { MenuRow, Popover } from "./popover";

type MenuView = "actions" | "rename" | "delete";

export function ConversationMenu({
  visible,
  conversation,
  onClose,
  onRename,
  onToggleStar,
  onDelete,
}: {
  visible: boolean;
  conversation: Conversation;
  onClose: () => void;
  onRename: (title: string) => void;
  onToggleStar: () => void;
  onDelete: () => void;
}) {
  const insets = useSafeAreaInsets();
  const [title, setTitle] = useState(conversation.title);
  const [view, setView] = useState<MenuView>("actions");

  useEffect(() => {
    if (!visible) return;
    setTitle(conversation.title);
    setView("actions");
  }, [visible, conversation.id]);

  const nextTitle = title.trim();
  const canRename = nextTitle.length > 0 && nextTitle !== conversation.title;

  function rename() {
    if (!canRename) return;
    tapFeedback();
    onRename(nextTitle);
    onClose();
  }

  return (
    <Popover
      onClose={onClose}
      style={{ right: 12, top: insets.top + 54 }}
      visible={visible}
      width={248}
    >
      {view === "actions" ? (
        <View>
          <MenuRow
            icon={conversation.starred ? "star" : "star-outline"}
            label={
              conversation.starred ? "Remove from starred" : "Add to starred"
            }
            onPress={() => {
              tapFeedback();
              onToggleStar();
              onClose();
            }}
          />
          <MenuRow
            icon="pencil-outline"
            label="Rename"
            onPress={() => {
              tapFeedback();
              setView("rename");
            }}
          />
          <MenuRow
            destructive
            icon="trash-outline"
            label="Delete"
            onPress={() => {
              tapFeedback();
              setView("delete");
            }}
          />
        </View>
      ) : null}

      {view === "rename" ? (
        <View style={styles.form}>
          <Text style={styles.label}>RENAME SESSION</Text>
          <View style={styles.inputWrap}>
            <TextInput
              accessibilityLabel="Session name"
              autoFocus
              onChangeText={setTitle}
              onSubmitEditing={rename}
              placeholder="Name this session"
              placeholderTextColor={palette.subtle}
              returnKeyType="done"
              selectionColor={palette.accent}
              style={styles.input}
              value={title}
            />
          </View>
          <View style={styles.actions}>
            <Pressable
              accessibilityLabel="Cancel rename"
              accessibilityRole="button"
              onPress={() => {
                tapFeedback();
                setTitle(conversation.title);
                setView("actions");
              }}
              style={({ pressed }) => [
                styles.secondary,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.secondaryLabel}>Cancel</Text>
            </Pressable>
            <Pressable
              accessibilityLabel="Save session name"
              accessibilityRole="button"
              accessibilityState={{ disabled: !canRename }}
              disabled={!canRename}
              onPress={rename}
              style={({ pressed }) => [
                styles.primary,
                !canRename && styles.primaryDisabled,
                pressed && styles.pressed,
              ]}
            >
              <Text
                style={[
                  styles.primaryLabel,
                  !canRename && styles.primaryLabelDisabled,
                ]}
              >
                Save
              </Text>
            </Pressable>
          </View>
        </View>
      ) : null}

      {view === "delete" ? (
        <View style={styles.form}>
          <Text style={styles.label}>DELETE SESSION</Text>
          <Text style={styles.body}>
            “{title.trim() || conversation.title}” will be removed from this
            phone. This cannot be undone.
          </Text>
          <View style={styles.actions}>
            <Pressable
              accessibilityLabel="Keep session"
              accessibilityRole="button"
              onPress={() => {
                tapFeedback();
                setView("actions");
              }}
              style={({ pressed }) => [
                styles.secondary,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.secondaryLabel}>Cancel</Text>
            </Pressable>
            <Pressable
              accessibilityLabel="Confirm delete session"
              accessibilityRole="button"
              onPress={() => {
                tapFeedback();
                onDelete();
              }}
              style={({ pressed }) => [
                styles.destructive,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.destructiveLabel}>Delete</Text>
            </Pressable>
          </View>
        </View>
      ) : null}
    </Popover>
  );
}

const styles = StyleSheet.create({
  form: {
    gap: space.sm,
    paddingHorizontal: space.lg,
    paddingVertical: space.sm,
  },
  label: {
    ...typography.eyebrow,
    color: palette.subtle,
    fontSize: 9,
  },
  inputWrap: {
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 10,
    paddingVertical: 8,
  },
  input: {
    ...typography.body,
    color: palette.text,
    fontSize: 14,
    outlineColor: "transparent",
    outlineStyle: "solid",
    padding: 0,
  },
  body: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
    lineHeight: 18,
  },
  actions: {
    flexDirection: "row",
    gap: 6,
    justifyContent: "flex-end",
    marginTop: 2,
  },
  secondary: {
    borderColor: palette.borderStrong,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  secondaryLabel: {
    ...typography.caption,
    color: palette.text,
    fontSize: 11,
    fontWeight: "600",
  },
  primary: {
    backgroundColor: palette.primary,
    borderRadius: radius.pill,
    paddingHorizontal: 14,
    paddingVertical: 7,
  },
  primaryDisabled: {
    backgroundColor: palette.surface,
  },
  primaryLabel: {
    ...typography.caption,
    color: palette.black,
    fontSize: 11,
    fontWeight: "600",
  },
  primaryLabelDisabled: {
    color: palette.subtle,
  },
  destructive: {
    backgroundColor: palette.error,
    borderRadius: radius.pill,
    paddingHorizontal: 14,
    paddingVertical: 7,
  },
  destructiveLabel: {
    ...typography.caption,
    color: palette.white,
    fontSize: 11,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.7,
  },
});
