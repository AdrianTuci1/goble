import { useNavigation } from "expo-router";

/** Open the nearest drawer navigator. The drawer adds `openDrawer` at runtime. */
export function useOpenDrawer(): () => void {
  const navigation = useNavigation();
  return () => {
    const drawer = navigation as unknown as { openDrawer?: () => void };
    drawer.openDrawer?.();
  };
}
