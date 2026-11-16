import { Ionicons } from "@expo/vector-icons";
import { Pressable, StyleSheet, TextInput, View } from "react-native";

import { palette, radius, space, typography } from "../../lib/theme";

/** A rounded search field that filters a settings list in place. */
export function SettingsSearchField({
  value,
  onChangeText,
  placeholder,
}: {
  value: string;
  onChangeText: (value: string) => void;
  placeholder: string;
}) {
  return (
    <View style={styles.field}>
      <Ionicons color={palette.muted} name="search" size={16} />
      <TextInput
        accessibilityLabel={placeholder}
        autoCapitalize="none"
        autoCorrect={false}
        onChangeText={onChangeText}
        placeholder={placeholder}
        placeholderTextColor={palette.subtle}
        returnKeyType="search"
        style={styles.input}
        value={value}
      />
      {value.length > 0 ? (
        <Pressable
          accessibilityLabel="Clear search"
          accessibilityRole="button"
          hitSlop={10}
          onPress={() => onChangeText("")}
        >
          <Ionicons color={palette.muted} name="close-circle" size={17} />
        </Pressable>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create({
  field: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.sm,
    minHeight: 48,
    paddingHorizontal: space.lg,
  },
  input: {
    ...typography.body,
    color: palette.text,
    flex: 1,
    fontSize: 15,
    padding: 0,
  },
});
