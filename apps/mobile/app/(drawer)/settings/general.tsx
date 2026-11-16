import { useRouter } from "expo-router";
import { useEffect, useState } from "react";

import {
  SettingsPage,
  SettingsRow,
  SettingsSection,
  SettingsSwitchRow,
  hostLabel,
  connection,
  useConnection,
} from "@goble/common";
import { signOut } from "../../../lib/api";

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
  const remote = useConnection();
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

  const linked = remote.phase === "connected" && remote.status === "live";
  const status =
    remote.phase === "pairing"
      ? "Pairing…"
      : remote.phase === "error"
        ? "Pairing failed"
        : remote.phase === "demo"
          ? "Demo"
          : linked
            ? "Live"
            : remote.status === "connecting"
              ? "Connecting…"
              : remote.workers.length === 0
                ? "No worker"
                : "Retrying…";
  const tone =
    linked ? "success" : remote.phase === "error" ? "error" : "warning";

  return (
    <SettingsPage title="General">
      <SettingsSection
        footer="The gateway runs the agents. This phone connects to it and keeps its token in the system keychain."
        title="Gateway"
      >
        <SettingsRow
          detail={hostLabel(remote.server ?? snapshot.server.url)}
          icon="server-outline"
          label="Server"
        />
        <SettingsRow
          detail={status}
          detailTone={tone}
          icon="pulse-outline"
          label="Status"
        />
        <SettingsRow
          description={
            remote.phase === "demo"
              ? "Pair with the machine that runs the agents"
              : remote.workerId
                ? `${remote.workers.length} connected`
                : "None connected"
          }
          detail={remote.workerId ?? "—"}
          icon="cube-outline"
          label="Worker"
          onPress={
            remote.workers.length > 1
              ? () => router.push("/settings/workers")
              : undefined
          }
        />
        {remote.phase === "connected" ? (
          <SettingsRow
            description="Reload the worker list and the relay socket"
            icon="refresh-outline"
            label="Reconnect"
            onPress={() => void connection.connect()}
          />
        ) : null}
        {remote.phase === "error" && remote.error ? (
          <SettingsRow
            description={remote.error}
            icon="alert-circle-outline"
            label="Last error"
          />
        ) : null}
        <SettingsRow
          description={
            remote.phase === "demo" ? "Pair a gateway" : "Pair another gateway"
          }
          icon="swap-horizontal-outline"
          label="Switch server"
          onPress={() => router.push("/sign-in")}
        />
        {remote.phase !== "demo" ? (
          <SettingsRow
            description="Forget the session and return to the demo workspace"
            icon="unlink-outline"
            label="Disconnect"
            onPress={async () => {
              await connection.disconnect();
              router.replace("/");
            }}
            tone="danger"
          />
        ) : null}
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
