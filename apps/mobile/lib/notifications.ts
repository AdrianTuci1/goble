import Constants from "expo-constants";
import * as SecureStore from "expo-secure-store";
import { Platform } from "react-native";

import { loadNotificationPreferences } from "./preferences";

type NotificationsModule = typeof import("expo-notifications");
export type NotificationKind = "run-finished" | "agent-needs-help";
export type NotificationPermission = "granted" | "denied" | "unsupported";

const PUSH_TOKEN_KEY = "goble.push_token";

let modulePromise: Promise<NotificationsModule | null> | null = null;
let configured = false;

/**
 * Loads expo-notifications on native only. The web preview keeps the settings
 * readable, but alerts are delivered by the installed app.
 */
async function notificationsModule(): Promise<NotificationsModule | null> {
  if (Platform.OS === "web") return null;
  modulePromise ??= import("expo-notifications").catch(() => null);
  return modulePromise;
}

async function configure(): Promise<NotificationsModule | null> {
  const notifications = await notificationsModule();
  if (!notifications || configured) return notifications;

  notifications.setNotificationHandler({
    handleNotification: async () => ({
      shouldPlaySound: true,
      shouldSetBadge: false,
      shouldShowBanner: true,
      shouldShowList: true,
    }),
  });

  if (Platform.OS === "android") {
    await Promise.all([
      notifications.setNotificationChannelAsync("runs", {
        importance: notifications.AndroidImportance.DEFAULT,
        lightColor: "#19aad8",
        name: "Runs",
      }),
      notifications.setNotificationChannelAsync("approvals", {
        importance: notifications.AndroidImportance.HIGH,
        lightColor: "#f59e0b",
        name: "Agent needs help",
      }),
    ]);
  }

  configured = true;
  return notifications;
}

export async function prepareNotifications(): Promise<NotificationPermission> {
  const notifications = await configure();
  if (!notifications) return "unsupported";
  const current = await notifications.getPermissionsAsync();
  return current.granted ? "granted" : "denied";
}

/** Asks for the system permission and registers an Expo push token. */
export async function ensureNotificationsEnabled(): Promise<{
  permission: NotificationPermission;
  token: string | null;
}> {
  const notifications = await configure();
  if (!notifications) return { permission: "unsupported", token: null };

  let permission = await notifications.getPermissionsAsync();
  if (!permission.granted && permission.canAskAgain) {
    permission = await notifications.requestPermissionsAsync();
  }
  if (!permission.granted) return { permission: "denied", token: null };

  return { permission: "granted", token: await registerPushToken() };
}

async function registerPushToken(): Promise<string | null> {
  const notifications = await notificationsModule();
  if (!notifications) return null;
  const extra = Constants.expoConfig?.extra as
    | { eas?: { projectId?: string } }
    | undefined;
  const projectId = Constants.easConfig?.projectId ?? extra?.eas?.projectId;
  try {
    const token = (
      await notifications.getExpoPushTokenAsync(
        projectId ? { projectId } : undefined,
      )
    ).data;
    await SecureStore.setItemAsync(PUSH_TOKEN_KEY, token);
    return token;
  } catch {
    // A development client has no EAS project yet; alerts still work locally.
    return null;
  }
}

/** Called once on launch: wires the handler and refreshes an existing token. */
export async function syncPushRegistration(): Promise<void> {
  const permission = await prepareNotifications();
  if (permission !== "granted") return;
  await registerPushToken();
}

export async function loadPushToken(): Promise<string | null> {
  if (Platform.OS === "web") return null;
  try {
    return await SecureStore.getItemAsync(PUSH_TOKEN_KEY);
  } catch {
    return null;
  }
}

async function deliver(
  kind: NotificationKind,
  title: string,
  body: string,
): Promise<void> {
  const notifications = await configure();
  if (!notifications) return;
  const permission = await notifications.getPermissionsAsync();
  if (!permission.granted) return;

  await notifications.scheduleNotificationAsync({
    content: {
      body,
      data: { kind },
      sound: "default",
      title,
    },
    trigger:
      Platform.OS === "android"
        ? { channelId: kind === "agent-needs-help" ? "approvals" : "runs" }
        : null,
  });
}

export async function notifyRunFinished(sessionTitle: string): Promise<void> {
  const preferences = await loadNotificationPreferences();
  if (!preferences.runFinished) return;
  await deliver(
    "run-finished",
    "Run finished",
    `${sessionTitle} finished on the gateway.`,
  );
}

export async function notifyAgentNeedsHelp(
  sessionTitle: string,
): Promise<void> {
  const preferences = await loadNotificationPreferences();
  if (!preferences.agentNeedsHelp) return;
  await deliver(
    "agent-needs-help",
    "Agent needs help",
    `${sessionTitle} is waiting for your approval.`,
  );
}

export async function sendTestNotifications(): Promise<NotificationPermission> {
  const { permission } = await ensureNotificationsEnabled();
  if (permission !== "granted") return permission;
  await Promise.all([
    notifyRunFinished("Gateway access control"),
    notifyAgentNeedsHelp("Gateway access control"),
  ]);
  return permission;
}
