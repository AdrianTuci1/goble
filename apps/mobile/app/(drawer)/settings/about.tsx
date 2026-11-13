import Constants from "expo-constants";
import { Platform, StyleSheet, Text, View } from "react-native";

import { BrandMark } from "../../../components/brand-mark";
import { SettingsPage } from "../../../components/settings/settings-page";
import { SettingsRow } from "../../../components/settings/settings-row";
import { SettingsSection } from "../../../components/settings/settings-section";
import { hostLabel } from "../../../lib/format";
import { palette, space, typography } from "../../../lib/theme";
import { useWorkspace } from "../../../lib/use-workspace";

export default function AboutSettingsScreen() {
  const { snapshot } = useWorkspace();
  const version = Constants.expoConfig?.version ?? "0.1.0";
  const sdk = Constants.expoConfig?.sdkVersion ?? "58";
  const status =
    snapshot.source === "live"
      ? "Online"
      : snapshot.source === "offline"
        ? "Offline"
        : "Demo";

  return (
    <SettingsPage title="About">
      <SettingsSection title="App">
        <SettingsRow detail={version} icon="cube-outline" label="Version" />
        <SettingsRow detail={sdk} icon="layers-outline" label="Expo SDK" />
        <SettingsRow
          detail={Platform.OS === "ios" ? "iOS" : Platform.OS === "android" ? "Android" : "Web"}
          icon="phone-portrait-outline"
          label="Platform"
        />
      </SettingsSection>

      <SettingsSection
        footer="The phone holds a device token, the preferences on these pages and a cache of the last snapshot. Sessions and keys stay on the gateway."
        title="Gateway"
      >
        <SettingsRow
          detail={hostLabel(snapshot.server.url)}
          icon="server-outline"
          label="Server"
        />
        <SettingsRow
          detail={status}
          detailTone={
            snapshot.source === "live"
              ? "success"
              : snapshot.source === "offline"
                ? "error"
                : "warning"
          }
          icon="pulse-outline"
          label="Status"
        />
        <SettingsRow
          detail={snapshot.server.version ?? "Unknown"}
          icon="git-branch-outline"
          label="Gateway version"
        />
      </SettingsSection>

      <View style={styles.brand}>
        <BrandMark size={34} />
        <View style={styles.brandText}>
          <Text style={styles.brandTitle}>goble mobile</Text>
          <Text style={styles.brandBody}>
            One workspace across desktop, browser and phone.
          </Text>
        </View>
      </View>
    </SettingsPage>
  );
}

const styles = StyleSheet.create({
  brand: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.lg,
    paddingHorizontal: space.sm,
    paddingTop: space.sm,
  },
  brandText: {
    flex: 1,
    gap: 2,
  },
  brandTitle: {
    ...typography.callout,
    color: palette.text,
  },
  brandBody: {
    ...typography.caption,
    color: palette.muted,
    lineHeight: 17,
  },
});
