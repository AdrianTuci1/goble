import type { ReactNode } from "react";
import {
  Modal,
  Pressable,
  StyleSheet,
  View,
  useWindowDimensions,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { palette, radius, space } from "../lib/theme";

/** Shared bottom sheet: overlay, grab handle and safe-area padding. */
export function Sheet({
  visible,
  onClose,
  children,
}: {
  visible: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const insets = useSafeAreaInsets();
  const { width } = useWindowDimensions();

  return (
    <Modal
      animationType="fade"
      onRequestClose={onClose}
      statusBarTranslucent
      transparent
      visible={visible}
    >
      <View style={styles.overlay}>
        <Pressable
          accessibilityLabel="Close sheet"
          onPress={onClose}
          style={StyleSheet.absoluteFill}
        />
        <View
          style={[
            styles.sheet,
            {
              marginHorizontal: width > 500 ? (width - 460) / 2 : 0,
              paddingBottom: Math.max(insets.bottom, space.lg),
            },
          ]}
        >
          <View style={styles.handle} />
          {children}
        </View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  overlay: {
    backgroundColor: "rgba(0,0,0,0.66)",
    flex: 1,
    justifyContent: "flex-end",
  },
  sheet: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderTopLeftRadius: radius.xl,
    borderTopRightRadius: radius.xl,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: space.lg,
    paddingTop: space.sm,
  },
  handle: {
    alignSelf: "center",
    backgroundColor: palette.borderStrong,
    borderRadius: radius.pill,
    height: 4,
    marginBottom: space.lg,
    width: 38,
  },
});
