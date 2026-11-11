import { Ionicons } from "@expo/vector-icons";
import { useRouter } from "expo-router";
import { useState } from "react";
import {
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { Screen } from "../../components/screen";
import { Sheet } from "../../components/sheet";
import { tapFeedback } from "../../lib/haptics";
import { demoAgents, demoModels } from "../../lib/mock";
import { palette, radius, space, typography } from "../../lib/theme";
import type { Agent } from "../../lib/types";

const tools = ["read/write", "read-only", "sandbox"] as const;

export default function AgentsScreen() {
  const router = useRouter();
  const models = demoModels();
  const [agents, setAgents] = useState<Agent[]>(() => demoAgents());
  const [sheetOpen, setSheetOpen] = useState(false);
  const [editing, setEditing] = useState<Agent | null>(null);
  const [draft, setDraft] = useState({
    model: models[0]?.name ?? "GPT-4o",
    name: "",
    summary: "",
    tools: tools[0] as string,
  });

  function openNew() {
    tapFeedback();
    setEditing(null);
    setDraft({
      model: models[0]?.name ?? "GPT-4o",
      name: "",
      summary: "",
      tools: tools[0],
    });
    setSheetOpen(true);
  }

  function openEdit(agent: Agent) {
    tapFeedback();
    setEditing(agent);
    setDraft({
      model: agent.model,
      name: agent.name,
      summary: agent.summary,
      tools: agent.tools,
    });
    setSheetOpen(true);
  }

  function close() {
    setSheetOpen(false);
    setEditing(null);
  }

  function save() {
    const name = draft.name.trim();
    if (!name) return;
    tapFeedback();
    const summary = draft.summary.trim() || "Custom agent for this workspace.";
    setAgents((current) =>
      editing
        ? current.map((agent) =>
            agent.id === editing.id
              ? {
                  ...agent,
                  model: draft.model,
                  name,
                  summary,
                  tools: draft.tools,
                }
              : agent,
          )
        : [
            {
              id: `agent-${Date.now()}`,
              model: draft.model,
              name,
              status: "ready",
              summary,
              tools: draft.tools,
            },
            ...current,
          ],
    );
    close();
  }

  return (
    <Screen>
      <View style={styles.header}>
        <View style={styles.side}>
          <Pressable
            accessibilityLabel="Back"
            accessibilityRole="button"
            hitSlop={8}
            onPress={() => {
              tapFeedback();
              if (router.canGoBack()) router.back();
              else router.replace("/");
            }}
            style={({ pressed }) => [
              styles.circle,
              pressed && styles.pressed,
            ]}
          >
            <Ionicons color={palette.text} name="chevron-back" size={20} />
          </Pressable>
        </View>
        <Text style={styles.title}>Agents</Text>
        <View style={[styles.side, styles.sideEnd]}>
          <Pressable
            accessibilityLabel="New agent"
            accessibilityRole="button"
            hitSlop={8}
            onPress={openNew}
            style={({ pressed }) => [
              styles.circle,
              pressed && styles.pressed,
            ]}
          >
            <Ionicons color={palette.text} name="pencil-outline" size={19} />
          </Pressable>
        </View>
      </View>

      <View style={styles.cards}>
        {agents.map((agent) => (
          <Pressable
            accessibilityLabel={`Edit ${agent.name}`}
            accessibilityRole="button"
            key={agent.id}
            onPress={() => openEdit(agent)}
            style={({ pressed }) => [styles.card, pressed && styles.cardPressed]}
          >
            <Ionicons
              color={palette.text}
              name={
                agent.tools === "sandbox"
                  ? "code-slash-outline"
                  : "reader-outline"
              }
              size={22}
            />
            <View style={styles.cardText}>
              <Text numberOfLines={1} style={styles.name}>
                {agent.name}
              </Text>
              <Text numberOfLines={1} style={styles.meta}>
                {agent.model} · {agent.tools}
                {agent.status === "paused" ? " · Paused" : ""}
              </Text>
            </View>
          </Pressable>
        ))}
      </View>

      <Sheet onClose={close} visible={sheetOpen}>
        <Text style={styles.sheetTitle}>
          {editing ? "Edit agent" : "New agent"}
        </Text>

        <Text style={styles.fieldLabel}>NAME</Text>
        <TextInput
          accessibilityLabel="Agent name"
          onChangeText={(name) =>
            setDraft((current) => ({ ...current, name }))
          }
          placeholder="Agent name"
          placeholderTextColor={palette.subtle}
          selectionColor={palette.accent}
          style={styles.input}
          value={draft.name}
        />

        <Text style={styles.fieldLabel}>SUMMARY</Text>
        <TextInput
          accessibilityLabel="Agent summary"
          multiline
          onChangeText={(summary) =>
            setDraft((current) => ({ ...current, summary }))
          }
          placeholder="What should this agent do?"
          placeholderTextColor={palette.subtle}
          selectionColor={palette.accent}
          style={[styles.input, styles.summaryInput]}
          textAlignVertical="top"
          value={draft.summary}
        />

        <Text style={styles.fieldLabel}>MODEL</Text>
        <View style={styles.chips}>
          {models.map((model) => {
            const active = draft.model === model.name;
            return (
              <Pressable
                accessibilityLabel={`Use ${model.name} for this agent`}
                accessibilityRole="button"
                accessibilityState={{ selected: active }}
                key={model.id}
                onPress={() => {
                  tapFeedback();
                  setDraft((current) => ({
                    ...current,
                    model: model.name,
                  }));
                }}
                style={({ pressed }) => [
                  styles.chip,
                  active && styles.chipActive,
                  pressed && styles.pressed,
                ]}
              >
                <Text
                  style={[styles.chipLabel, active && styles.chipLabelActive]}
                >
                  {model.name}
                </Text>
              </Pressable>
            );
          })}
        </View>

        <Text style={styles.fieldLabel}>TOOLS</Text>
        <View style={styles.chips}>
          {tools.map((tool) => {
            const active = draft.tools === tool;
            return (
              <Pressable
                accessibilityLabel={`Use ${tool} tools`}
                accessibilityRole="button"
                accessibilityState={{ selected: active }}
                key={tool}
                onPress={() => {
                  tapFeedback();
                  setDraft((current) => ({ ...current, tools: tool }));
                }}
                style={({ pressed }) => [
                  styles.chip,
                  active && styles.chipActive,
                  pressed && styles.pressed,
                ]}
              >
                <Text
                  style={[styles.chipLabel, active && styles.chipLabelActive]}
                >
                  {tool}
                </Text>
              </Pressable>
            );
          })}
        </View>

        <Pressable
          accessibilityLabel={editing ? "Save agent" : "Add agent"}
          accessibilityRole="button"
          accessibilityState={{ disabled: !draft.name.trim() }}
          disabled={!draft.name.trim()}
          onPress={save}
          style={({ pressed }) => [
            styles.primary,
            !draft.name.trim() && styles.disabled,
            pressed && styles.pressed,
          ]}
        >
          <Text style={styles.primaryLabel}>
            {editing ? "Save agent" : "Add agent"}
          </Text>
        </Pressable>
      </Sheet>
    </Screen>
  );
}

const styles = StyleSheet.create({
  header: {
    alignItems: "center",
    flexDirection: "row",
    paddingBottom: space.lg,
    paddingTop: space.sm,
  },
  side: {
    flex: 1,
    flexDirection: "row",
  },
  sideEnd: {
    justifyContent: "flex-end",
  },
  title: {
    ...typography.title,
    color: palette.text,
    fontSize: 20,
    textAlign: "center",
  },
  circle: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  cards: {
    gap: 14,
  },
  card: {
    alignItems: "center",
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: 22,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.lg,
    minHeight: 78,
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
  },
  cardPressed: {
    backgroundColor: palette.raised,
  },
  cardText: {
    flex: 1,
    gap: 4,
    minWidth: 0,
  },
  name: {
    ...typography.callout,
    color: palette.text,
    fontSize: 16,
    fontWeight: "400",
  },
  meta: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  sheetTitle: {
    ...typography.title,
    color: palette.text,
    fontSize: 22,
    marginBottom: space.lg,
  },
  fieldLabel: {
    ...typography.eyebrow,
    color: palette.muted,
    fontSize: 9,
    marginBottom: space.xs,
    marginTop: space.md,
  },
  input: {
    ...typography.body,
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    color: palette.text,
    fontSize: 14,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingHorizontal: 10,
    paddingVertical: 10,
  },
  summaryInput: {
    minHeight: 68,
  },
  chips: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  chip: {
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  chipActive: {
    backgroundColor: palette.accentSoft,
    borderColor: palette.borderStrong,
  },
  chipLabel: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
  },
  chipLabelActive: {
    color: palette.text,
  },
  primary: {
    alignItems: "center",
    backgroundColor: palette.primary,
    borderRadius: radius.pill,
    marginBottom: space.sm,
    marginTop: space.xl,
    paddingVertical: 12,
  },
  primaryLabel: {
    ...typography.callout,
    color: palette.black,
    fontSize: 14,
    fontWeight: "600",
  },
  disabled: {
    opacity: 0.4,
  },
  pressed: {
    opacity: 0.7,
  },
});
