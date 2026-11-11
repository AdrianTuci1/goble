import { Ionicons } from "@expo/vector-icons";
import Constants from "expo-constants";
import { useRouter } from "expo-router";
import { useEffect, useState } from "react";
import { Pressable, StyleSheet, Switch, Text, View } from "react-native";
import type { ComponentProps } from "react";

import { PageHeader } from "../../components/page-header";
import { Screen } from "../../components/screen";
import { StatusPill } from "../../components/status-pill";
import { signOut } from "../../lib/api";
import { hostLabel } from "../../lib/format";
import { tapFeedback } from "../../lib/haptics";
import { demoModels } from "../../lib/mock";
import {
  ensureNotificationsEnabled,
  prepareNotifications,
  sendTestNotifications,
  type NotificationPermission,
} from "../../lib/notifications";
import {
  DEFAULT_MODEL_PREFERENCES,
  DEFAULT_NOTIFICATION_PREFERENCES,
  loadModelPreferences,
  loadNotificationPreferences,
  loadVoicePreferences,
  saveModelPreferences,
  saveNotificationPreferences,
  subscribeModelPreferences,
  subscribeNotificationPreferences,
  subscribeVoicePreferences,
  type ModelPreferences,
  type NotificationPreferences,
} from "../../lib/preferences";
import { palette, radius, space, typography } from "../../lib/theme";
import type { Model } from "../../lib/types";
import { useWorkspace } from "../../lib/use-workspace";
import {
  DEFAULT_VOICE_PREFERENCES,
  voiceProvider,
  type VoicePreferences,
} from "../../lib/voice";

type IconName = ComponentProps<typeof Ionicons>["name"];

export default function SettingsScreen() {
  const router = useRouter();
  const { snapshot } = useWorkspace();
  const [notifications, setNotifications] = useState<NotificationPreferences>(
    DEFAULT_NOTIFICATION_PREFERENCES,
  );
  const [permission, setPermission] =
    useState<NotificationPermission | null>(null);
  const [modelPreferences, setModelPreferences] = useState<ModelPreferences>(
    DEFAULT_MODEL_PREFERENCES,
  );
  const [haptics, setHaptics] = useState(true);
  const [voice, setVoice] = useState<VoicePreferences>(
    DEFAULT_VOICE_PREFERENCES,
  );

  useEffect(() => {
    void loadVoicePreferences().then(setVoice);
    return subscribeVoicePreferences(setVoice);
  }, []);

  useEffect(() => {
    void loadNotificationPreferences().then(setNotifications);
    void prepareNotifications().then(setPermission);
    return subscribeNotificationPreferences(setNotifications);
  }, []);

  useEffect(() => {
    void loadModelPreferences().then(setModelPreferences);
    return subscribeModelPreferences(setModelPreferences);
  }, []);

  function updateNotifications(next: NotificationPreferences) {
    setNotifications(next);
    void saveNotificationPreferences(next);
  }

  async function toggleNotification(
    key: "runFinished" | "agentNeedsHelp",
    value: boolean,
  ) {
    const previous = notifications;
    const next = { ...notifications, [key]: value };
    updateNotifications(next);
    if (!value) return;
    const result = await ensureNotificationsEnabled();
    setPermission(result.permission);
    if (result.permission === "denied") {
      updateNotifications({ ...previous, [key]: false });
    }
  }

  async function sendTest() {
    const result = await sendTestNotifications();
    setPermission(result);
  }

  function updateModels(next: ModelPreferences) {
    setModelPreferences(next);
    void saveModelPreferences(next);
  }

  function toggleModel(model: Model, enabled: boolean) {
    tapFeedback();
    const disabled = new Set(modelPreferences.disabled);
    if (enabled) disabled.delete(model.id);
    else disabled.add(model.id);
    const remaining = demoModels().filter((entry) => !disabled.has(entry.id));
    if (remaining.length === 0) return;
    const currentDefault = modelPreferences.defaultModelId;
    const defaultModelId =
      currentDefault && disabled.has(currentDefault)
        ? remaining[0]!.id
        : currentDefault;
    updateModels({ defaultModelId, disabled: [...disabled] });
  }

  function makeDefault(model: Model) {
    tapFeedback();
    updateModels({
      defaultModelId: model.id,
      disabled: modelPreferences.disabled.filter((id) => id !== model.id),
    });
  }

  const notificationNote =
    permission === "unsupported"
      ? "Alerts are delivered by the iOS and Android app. The browser preview cannot show them."
      : permission === "denied"
        ? "Notifications are blocked in system settings."
        : permission === "granted"
          ? "This phone is registered for run and approval alerts."
          : "Goble alerts you when a run finishes or an agent needs help.";

  const connection =
    snapshot.source === "live"
      ? "online"
      : snapshot.source === "offline"
        ? "offline"
        : "demo";

  const catalog = demoModels();
  const enabledCatalog = catalog.filter(
    (model) => !modelPreferences.disabled.includes(model.id),
  );
  const defaultModelId =
    modelPreferences.defaultModelId &&
    enabledCatalog.some((model) => model.id === modelPreferences.defaultModelId)
      ? modelPreferences.defaultModelId
      : (enabledCatalog[0] ?? catalog[0]!).id;

  return (
    <Screen>
      <PageHeader eyebrow="WORKSPACE" title="Settings" />

      <View style={styles.section}>
        <Text style={styles.sectionTitle}>GATEWAY</Text>
        <Pressable
          accessibilityRole="button"
          onPress={() => {
            tapFeedback();
            router.push("/sign-in");
          }}
          style={({ pressed }) => [styles.card, pressed && styles.pressed]}
        >
          <View style={styles.cardText}>
            <Text numberOfLines={1} style={styles.cardTitle}>
              {snapshot.server.name}
            </Text>
            <Text numberOfLines={1} style={styles.cardBody}>
              {snapshot.source === "demo"
                ? "Demo data on this device"
                : hostLabel(snapshot.server.url)}
            </Text>
          </View>
          <StatusPill
            label={
              snapshot.source === "live"
                ? "Online"
                : snapshot.source === "offline"
                  ? "Offline"
                  : "Demo"
            }
            state={connection}
          />
        </Pressable>
      </View>

      <View style={styles.section}>
        <Text style={styles.sectionTitle}>MODELS</Text>
        <View style={styles.group}>
          {catalog.map((model, index) => (
            <View key={model.id}>
              {index > 0 ? <View style={styles.divider} /> : null}
              <ModelRow
                enabled={!modelPreferences.disabled.includes(model.id)}
                isDefault={defaultModelId === model.id}
                model={model}
                onSelect={() => makeDefault(model)}
                onToggle={(next) => toggleModel(model, next)}
              />
            </View>
          ))}
        </View>
        <Text style={styles.note}>
          Enabled models appear in the top bar. Tap a model to make it the
          default for new sessions.
        </Text>
      </View>

      <View style={styles.section}>
        <Text style={styles.sectionTitle}>NOTIFICATIONS</Text>
        <View style={styles.group}>
          <SettingSwitch
            detail="When a run completes on the gateway."
            icon="checkmark-done-outline"
            label="Run finished"
            onValueChange={(next) => void toggleNotification("runFinished", next)}
            value={notifications.runFinished}
          />
          <View style={styles.divider} />
          <SettingSwitch
            detail="When an agent is waiting for an approval."
            icon="help-buoy-outline"
            label="Agent needs help"
            onValueChange={(next) =>
              void toggleNotification("agentNeedsHelp", next)
            }
            value={notifications.agentNeedsHelp}
          />
          <View style={styles.divider} />
          <SettingRow
            chevron={false}
            icon="paper-plane-outline"
            label="Send a test notification"
            detail="Fires the enabled run and approval alerts."
            onPress={() => void sendTest()}
          />
        </View>
        <Text style={styles.note}>{notificationNote}</Text>
      </View>

      <View style={styles.section}>
        <Text style={styles.sectionTitle}>PREFERENCES</Text>
        <View style={styles.group}>
          <SettingSwitch
            icon="phone-portrait-outline"
            label="Haptic feedback"
            onValueChange={setHaptics}
            value={haptics}
          />
        </View>
      </View>

      <View style={styles.section}>
        <Text style={styles.sectionTitle}>VOICE</Text>
        <View style={styles.group}>
          <SettingRow
            icon="mic-outline"
            label="Voice models"
            detail={`${voiceProvider(voice.provider).name} · ${voice.ttsModel}`}
            onPress={() => router.push("/voice")}
          />
        </View>
      </View>

      <View style={styles.section}>
        <Text style={styles.sectionTitle}>ACCOUNT</Text>
        <View style={styles.group}>
          <SettingRow
            icon="person-circle-outline"
            label="Switch server"
            detail="Sign in to another gateway"
            onPress={() => router.push("/sign-in")}
          />
          <View style={styles.divider} />
          <SettingRow
            icon="key-outline"
            label="Device access"
            detail="Tokens stay in the system keychain"
            onPress={() => undefined}
          />
          <View style={styles.divider} />
          <SettingRow
            icon="log-out-outline"
            label="Sign out"
            detail="Forget this gateway on the phone"
            onPress={async () => {
              await signOut();
              router.replace("/sign-in");
            }}
          />
        </View>
      </View>

      <View style={styles.about}>
        <Text style={styles.aboutBrand}>goble mobile</Text>
        <Text style={styles.aboutBody}>
          Version {Constants.expoConfig?.version ?? "0.1.0"} · Expo{" "}
          {Constants.expoConfig?.sdkVersion ?? "58"}
        </Text>
        <Text style={styles.aboutBody}>
          One workspace across desktop, browser and phone.
        </Text>
      </View>
    </Screen>
  );
}

function SettingSwitch({
  icon,
  label,
  detail,
  value,
  onValueChange,
}: {
  icon: IconName;
  label: string;
  detail?: string;
  value: boolean;
  onValueChange: (value: boolean) => void;
}) {
  return (
    <View style={styles.row}>
      <Ionicons name={icon} size={17} color={palette.muted} />
      <View style={styles.rowText}>
        <Text style={styles.rowLabel}>{label}</Text>
        {detail ? <Text style={styles.rowDetail}>{detail}</Text> : null}
      </View>
      <Switch
        accessibilityLabel={label}
        ios_backgroundColor={palette.raised}
        onValueChange={(next) => {
          tapFeedback();
          onValueChange(next);
        }}
        thumbColor={value ? palette.text : palette.muted}
        trackColor={{ false: palette.raised, true: palette.accentSoft }}
        value={value}
      />
    </View>
  );
}

function ModelRow({
  model,
  isDefault,
  enabled,
  onSelect,
  onToggle,
}: {
  model: Model;
  isDefault: boolean;
  enabled: boolean;
  onSelect: () => void;
  onToggle: (enabled: boolean) => void;
}) {
  return (
    <View style={styles.row}>
      <Pressable
        accessibilityLabel={`Use ${model.name} as the default model`}
        accessibilityRole="button"
        accessibilityState={{ selected: isDefault }}
        onPress={onSelect}
        style={({ pressed }) => [styles.modelMain, pressed && styles.pressed]}
      >
        <Ionicons
          color={isDefault ? palette.accent : palette.subtle}
          name={isDefault ? "checkmark-circle" : "ellipse-outline"}
          size={19}
        />
        <View style={styles.rowText}>
          <Text style={styles.rowLabel}>{model.name}</Text>
          <Text style={styles.rowDetail}>
            {model.vendor} · {model.detail}
          </Text>
        </View>
      </Pressable>
      <Switch
        accessibilityLabel={`Enable ${model.name}`}
        ios_backgroundColor={palette.raised}
        onValueChange={(next) => {
          tapFeedback();
          onToggle(next);
        }}
        thumbColor={enabled ? palette.text : palette.muted}
        trackColor={{ false: palette.raised, true: palette.accentSoft }}
        value={enabled}
      />
    </View>
  );
}

function SettingRow({
  icon,
  label,
  detail,
  chevron = true,
  onPress,
}: {
  icon: IconName;
  label: string;
  detail: string;
  chevron?: boolean;
  onPress: () => void;
}) {
  return (
    <Pressable
      accessibilityRole="button"
      onPress={() => {
        tapFeedback();
        onPress();
      }}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
    >
      <Ionicons name={icon} size={17} color={palette.muted} />
      <View style={styles.rowText}>
        <Text style={styles.rowLabel}>{label}</Text>
        <Text style={styles.rowDetail}>{detail}</Text>
      </View>
      {chevron ? (
        <Ionicons name="chevron-forward" size={15} color={palette.subtle} />
      ) : null}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  section: {
    marginBottom: space.xl,
  },
  sectionTitle: {
    ...typography.eyebrow,
    color: palette.muted,
    marginBottom: space.sm,
    paddingLeft: space.xs,
  },
  card: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.md,
    padding: space.md,
  },
  cardText: {
    flex: 1,
    gap: 2,
  },
  cardTitle: {
    ...typography.callout,
    color: palette.text,
  },
  cardBody: {
    ...typography.caption,
    color: palette.muted,
  },
  group: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: "hidden",
  },
  row: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    minHeight: 54,
    paddingHorizontal: space.md,
    paddingVertical: space.sm,
  },
  rowText: {
    flex: 1,
    gap: 2,
    minWidth: 0,
  },
  modelMain: {
    alignItems: "center",
    flex: 1,
    flexDirection: "row",
    gap: space.md,
  },
  rowLabel: {
    ...typography.callout,
    color: palette.text,
    flex: 1,
  },
  rowDetail: {
    ...typography.caption,
    color: palette.muted,
  },
  divider: {
    backgroundColor: palette.border,
    height: StyleSheet.hairlineWidth,
    marginLeft: 44,
  },
  note: {
    ...typography.caption,
    color: palette.subtle,
    lineHeight: 17,
    marginTop: space.sm,
    paddingLeft: space.xs,
  },
  pressed: {
    opacity: 0.68,
  },
  about: {
    gap: space.xs,
    paddingBottom: space.xl,
    paddingTop: space.md,
  },
  aboutBrand: {
    ...typography.callout,
    color: palette.muted,
  },
  aboutBody: {
    ...typography.caption,
    color: palette.subtle,
  },
});
