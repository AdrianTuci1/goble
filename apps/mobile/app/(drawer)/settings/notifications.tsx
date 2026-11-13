import { useEffect, useState } from "react";

import { SettingsPage } from "../../../components/settings/settings-page";
import { SettingsRow } from "../../../components/settings/settings-row";
import { SettingsSection } from "../../../components/settings/settings-section";
import { SettingsSwitchRow } from "../../../components/settings/settings-switch-row";
import {
  ensureNotificationsEnabled,
  prepareNotifications,
  sendTestNotifications,
  type NotificationPermission,
} from "../../../lib/notifications";
import {
  DEFAULT_NOTIFICATION_PREFERENCES,
  loadNotificationPreferences,
  saveNotificationPreferences,
  subscribeNotificationPreferences,
  type NotificationPreferences,
} from "../../../lib/preferences";

const permissionLabel: Record<NotificationPermission, string> = {
  denied: "Blocked",
  granted: "Allowed",
  unsupported: "Unavailable here",
};

export default function NotificationsSettingsScreen() {
  const [preferences, setPreferences] = useState<NotificationPreferences>(
    DEFAULT_NOTIFICATION_PREFERENCES,
  );
  const [permission, setPermission] =
    useState<NotificationPermission | null>(null);

  useEffect(() => {
    void loadNotificationPreferences().then(setPreferences);
    void prepareNotifications().then(setPermission);
    return subscribeNotificationPreferences(setPreferences);
  }, []);

  function update(next: NotificationPreferences) {
    setPreferences(next);
    void saveNotificationPreferences(next);
  }

  async function toggle(
    key: "runFinished" | "agentNeedsHelp",
    value: boolean,
  ) {
    const previous = preferences;
    update({ ...preferences, [key]: value });
    if (!value) return;
    const result = await ensureNotificationsEnabled();
    setPermission(result.permission);
    if (result.permission === "denied") {
      update({ ...previous, [key]: false });
    }
  }

  async function sendTest() {
    const result = await sendTestNotifications();
    setPermission(result);
  }

  const note =
    permission === "unsupported"
      ? "Alerts are delivered by the iOS and Android app. The browser preview cannot show them."
      : permission === "denied"
        ? "Notifications are blocked in the system settings of this phone."
        : permission === "granted"
          ? "This phone is registered for run and approval alerts."
          : "Goble alerts you when a run finishes or an agent needs help.";

  return (
    <SettingsPage title="Notifications">
      <SettingsSection
        footer="Alerts arrive on this phone, never on the gateway."
        title="Alerts"
      >
        <SettingsSwitchRow
          description="When a run completes on the gateway."
          icon="checkmark-done-outline"
          label="Run finished"
          onValueChange={(next) => void toggle("runFinished", next)}
          value={preferences.runFinished}
        />
        <SettingsSwitchRow
          description="When an agent is waiting for an approval."
          icon="help-buoy-outline"
          label="Agent needs help"
          onValueChange={(next) => void toggle("agentNeedsHelp", next)}
          value={preferences.agentNeedsHelp}
        />
      </SettingsSection>

      <SettingsSection footer={note} title="This phone">
        <SettingsRow
          detail={
            permission ? permissionLabel[permission] : "Checking"
          }
          icon="shield-checkmark-outline"
          label="Permission"
        />
        <SettingsRow
          chevron={false}
          description="Fires the enabled run and approval alerts."
          icon="paper-plane-outline"
          label="Send a test notification"
          onPress={() => void sendTest()}
        />
      </SettingsSection>
    </SettingsPage>
  );
}
