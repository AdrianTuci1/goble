import { Ionicons } from "@expo/vector-icons";
import type { ComponentProps, ReactNode } from "react";
import {
  Modal,
  Pressable,
  StyleSheet,
  Text,
  View,
  type StyleProp,
  type ViewStyle,
} from "react-native";

import { palette, radius, space, withAlpha } from "../lib/theme";

type IconName = ComponentProps<typeof Ionicons>["name"];

/** A floating menu anchored to a control, styled like the Gemini overflow. */
export function Popover({
  visible,
  onClose,
  width = 248,
  style,
  children,
}: {
  visible: boolean;
  onClose: () => void;
  width?: number;
  style?: StyleProp<ViewStyle>;
  children: ReactNode;
}) {
  return (
    <Modal
      animationType="fade"
      onRequestClose={onClose}
      statusBarTranslucent
      transparent
      visible={visible}
    >
      <Pressable
        accessibilityLabel="Close menu"
        onPress={onClose}
        style={StyleSheet.absoluteFill}
      />
      <View style={[styles.card, { width }, style]}>{children}</View>
    </Modal>
  );
}

export function MenuRow({
  icon,
  label,
  onPress,
  destructive = false,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
  destructive?: boolean;
}) {
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.rowPressed]}
    >
      <View style={styles.rowIcon}>
        <Ionicons
          color={destructive ? palette.error : palette.text}
          name={icon}
          size={20}
        />
      </View>
      <Text style={[styles.rowLabel, destructive && styles.destructive]}>
        {label}
      </Text>
    </Pressable>
  );
}

export function MenuDivider() {
  return <View style={styles.divider} />;
}

const styles = StyleSheet.create({
  divider: {
    backgroundColor: withAlpha(palette.white, 0.08),
    height: StyleSheet.hairlineWidth,
    marginHorizontal: space.lg,
    marginVertical: 6,
  },
  card: {
    backgroundColor: palette.overlay,
    borderColor: withAlpha(palette.white, 0.07),
    borderRadius: 22,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: "hidden",
    paddingVertical: 7,
    position: "absolute",
    shadowColor: palette.black,
    shadowOffset: { height: 18, width: 0 },
    shadowOpacity: 0.5,
    shadowRadius: 32,
  },
  row: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    minHeight: 50,
    paddingHorizontal: space.lg,
  },
  rowPressed: {
    backgroundColor: withAlpha(palette.white, 0.06),
  },
  rowIcon: {
    alignItems: "center",
    height: 24,
    justifyContent: "center",
    width: 24,
  },
  rowLabel: {
    color: palette.text,
    flex: 1,
    fontSize: 16,
    lineHeight: 21,
  },
  destructive: {
    color: palette.error,
  },
});
