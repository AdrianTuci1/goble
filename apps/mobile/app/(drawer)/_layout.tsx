import { Drawer } from "expo-router/drawer";

import { Sidebar } from "../../components/sidebar";
import { palette } from "../../lib/theme";

export default function DrawerLayout() {
  return (
    <Drawer
      drawerContent={(props) => (
        <Sidebar onClose={() => props.navigation.closeDrawer()} />
      )}
      screenOptions={{
        headerShown: false,
        drawerType: "front",
        drawerStyle: {
          backgroundColor: palette.bg,
          width: "100%",
        },
        overlayColor: "rgba(0,0,0,0.62)",
        swipeEdgeWidth: 64,
        sceneStyle: { backgroundColor: palette.bg },
        drawerActiveTintColor: palette.text,
        drawerInactiveTintColor: palette.muted,
      }}
    >
      <Drawer.Screen name="index" options={{ title: "Home" }} />
      <Drawer.Screen name="agents" options={{ title: "Agents" }} />
      <Drawer.Screen name="scheduled" options={{ title: "Scheduled" }} />
      <Drawer.Screen name="customize" options={{ title: "Customize" }} />
      <Drawer.Screen name="voice" options={{ title: "Voice" }} />
      <Drawer.Screen name="settings" options={{ title: "Settings" }} />
      <Drawer.Screen name="session/[id]" options={{ title: "Session" }} />
    </Drawer>
  );
}
