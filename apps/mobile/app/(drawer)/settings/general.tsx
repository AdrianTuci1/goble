import { useRouter } from "expo-router";
import { useEffect, useState } from "react";

import { SettingsPage } from "../../../components/settings/settings-page";
import { SettingsRow } from "../../../components/settings/settings-row";
import { SettingsSection } from "../../../components/settings/settings-section";
import { SettingsSwitchRow } from "../../../components/settings/settings-switch-row";
import { signOut } from "../../../lib/api";
import { hostLabel } from "../../../lib/format";
import { setHapticsEnabled } from "../../../lib/haptics";
import {
  loadHapticsPreference,
  saveHapticsPreference,
  subscribeHapticsPreference,
} from "../../../lib/preferences";
import { useWorkspace } from "../../../lib/use-workspace";

export default function GeneralSettingsScreen() {
  const router = useRouter();
  const { snapshot } = useWorkspace();
  const [haptics, setHaptics] = useState(true);

  useEffect(() => {
    void loadHapticsPreference().then(setHaptics);
    return subscribeHapticsPreference(setHaptics);
  }, []);

  function updateHaptics(enabled: boolean) {
    setHaptics(enabled);
    setHapticsEnabled(enabled);
    void saveHapticsPreference(enabled);
  }

  const status =
    snapshot.source === "live"
      ? "Online"
      : snapshot.source === "offline"
        ? "Offline"
        : "Demo";

  return (
    <SettingsPage title="General">
      <SettingsSection
        footer="The gateway runs the agents. This phone connects to it and keeps its token in the system keychain."
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
          icon="cube-outline"
          label="Version"
        />
        <SettingsRow
          description="Sign in to another gateway"
          icon="swap-horizontal-outline"
          label="Switch server"
          onPress={() => router.push("/sign-in")}
        />
      </SettingsSection>

      <SettingsSection
        footer="A short tap when you pick a row, change a model or send a turn."
        title="Device"
      >
        <SettingsSwitchRow
          description="Vibrate on taps"
          icon="phone-portrait-outline"
          label="Haptic feedback"
          onValueChange={updateHaptics}
          value={haptics}
        />
      </SettingsSection>

      <SettingsSection footer="Sessions on the gateway stay where they are.">
        <SettingsRow
          icon="log-out-outline"
          label="Sign out"
          onPress={async () => {
            await signOut();
            router.replace("/sign-in");
          }}
          tone="danger"
        />
      </SettingsSection>
    </SettingsPage>
  );
}
