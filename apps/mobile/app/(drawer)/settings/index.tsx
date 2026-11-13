import { useRouter } from "expo-router";
import { useState } from "react";
import { StyleSheet, Text, View } from "react-native";

import { SettingsRootPage } from "../../../components/settings/settings-page";
import {
  SettingsRow,
  type SettingsIconName,
} from "../../../components/settings/settings-row";
import { SettingsSearchField } from "../../../components/settings/settings-search-field";
import { SettingsSection } from "../../../components/settings/settings-section";
import { StatusPill } from "../../../components/status-pill";
import { signOut } from "../../../lib/api";
import { hostLabel } from "../../../lib/format";
import { palette, space, typography } from "../../../lib/theme";
import { useModelChoices } from "../../../lib/use-models";
import { useVoicePreferences } from "../../../lib/use-voice";
import { useWorkspace } from "../../../lib/use-workspace";
import { voiceProvider } from "../../../lib/voice";

interface SettingsListRow {
  id: string;
  icon: SettingsIconName;
  label: string;
  description?: string;
  detail?: string;
  href?: string;
}

interface SettingsListSection {
  title?: string;
  footer?: string;
  rows: SettingsListRow[];
}

export default function SettingsScreen() {
  const router = useRouter();
  const { snapshot } = useWorkspace();
  const { allModels, models } = useModelChoices();
  const { preferences: voice } = useVoicePreferences();
  const [query, setQuery] = useState("");

  const sections: SettingsListSection[] = [
    {
      title: "Preferences",
      rows: [
        {
          description: "Gateway, account and haptics",
          href: "/settings/general",
          icon: "options-outline",
          id: "general",
          label: "General",
        },
        {
          description: "Mode, thinking and instructions",
          href: "/settings/chat",
          icon: "chatbubbles-outline",
          id: "chat",
          label: "Chat",
        },
        {
          description: "The catalog this phone can run",
          detail: `${models.length}/${allModels.length}`,
          href: "/settings/models",
          icon: "sparkles-outline",
          id: "models",
          label: "Models",
        },
        {
          description: "Provider, voices and spoken replies",
          detail: voiceProvider(voice.provider).name,
          href: "/settings/voice",
          icon: "mic-outline",
          id: "voice",
          label: "Voice",
        },
        {
          description: "Finished runs and approvals",
          href: "/settings/notifications",
          icon: "notifications-outline",
          id: "notifications",
          label: "Notifications",
        },
      ],
    },
    {
      title: "Agents",
      rows: [
        {
          description: "Custom agents and the models they run",
          href: "/agents",
          icon: "people-outline",
          id: "agents",
          label: "Agents",
        },
        {
          description: "What the agent can reach",
          href: "/customize",
          icon: "color-palette-outline",
          id: "customize",
          label: "Skills & connectors",
        },
        {
          description: "Work the gateway runs on its own",
          href: "/scheduled",
          icon: "calendar-outline",
          id: "scheduled",
          label: "Scheduled tasks",
        },
      ],
    },
    {
      title: "About",
      rows: [
        {
          description: "Version and device details",
          href: "/settings/about",
          icon: "information-circle-outline",
          id: "about",
          label: "About",
        },
      ],
    },
  ];

  const needle = query.trim().toLowerCase();
  const matches = (value?: string) =>
    value !== undefined && value.toLowerCase().includes(needle);
  const filtered = sections
    .map((section) => ({
      ...section,
      rows: section.rows.filter(
        (row) =>
          matches(row.label) ||
          matches(row.description) ||
          matches(row.detail) ||
          matches(section.title),
      ),
    }))
    .filter((section) => section.rows.length > 0);

  const connection =
    snapshot.source === "live"
      ? "online"
      : snapshot.source === "offline"
        ? "offline"
        : "demo";

  const connectionLabel =
    snapshot.source === "live"
      ? "Online"
      : snapshot.source === "offline"
        ? "Offline"
        : "Demo";

  return (
    <SettingsRootPage title="Settings">
      <SettingsSearchField
        onChangeText={setQuery}
        placeholder="Search settings"
        value={query}
      />

      <SettingsSection>
        <SettingsRow
          accessory={<StatusPill label={connectionLabel} state={connection} />}
          description={
            snapshot.source === "demo"
              ? "Demo data on this device"
              : hostLabel(snapshot.server.url)
          }
          icon="server-outline"
          label={snapshot.server.name}
          onPress={() => router.push("/settings/general")}
        />
      </SettingsSection>

      {filtered.map((section) => (
        <SettingsSection
          footer={section.footer}
          key={section.title ?? section.rows[0]!.id}
          title={section.title}
        >
          {section.rows.map((row) => (
            <SettingsRow
              description={row.description}
              detail={row.detail}
              icon={row.icon}
              key={row.id}
              label={row.label}
              onPress={() => {
                if (row.href) router.push(row.href);
              }}
            />
          ))}
        </SettingsSection>
      ))}

      {filtered.length === 0 ? (
        <View style={styles.empty}>
          <Text style={styles.emptyTitle}>No results</Text>
          <Text style={styles.emptyBody}>
            No settings match “{query.trim()}”.
          </Text>
        </View>
      ) : null}

      {needle.length === 0 ? (
        <SettingsSection
          footer="Forget this gateway on the phone. Sessions on the gateway stay where they are."
        >
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
      ) : null}

      <View style={styles.about}>
        <Text style={styles.aboutTitle}>goble mobile</Text>
        <Text style={styles.aboutBody}>
          One workspace across desktop, browser and phone.
        </Text>
      </View>
    </SettingsRootPage>
  );
}

const styles = StyleSheet.create({
  empty: {
    alignItems: "center",
    gap: space.sm,
    paddingHorizontal: space.xl,
    paddingVertical: space.xxl,
  },
  emptyTitle: {
    ...typography.title,
    color: palette.text,
    fontSize: 18,
  },
  emptyBody: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 14,
    lineHeight: 20,
    textAlign: "center",
  },
  about: {
    gap: space.xs,
    paddingBottom: space.lg,
    paddingHorizontal: space.sm,
  },
  aboutTitle: {
    ...typography.callout,
    color: palette.muted,
  },
  aboutBody: {
    ...typography.caption,
    color: palette.subtle,
    lineHeight: 17,
  },
});
