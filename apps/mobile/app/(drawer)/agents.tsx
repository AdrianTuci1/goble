import { Ionicons } from "@expo/vector-icons";
import { useState } from "react";
import { Pressable, StyleSheet, Text, TextInput, View } from "react-native";

import { SettingsButton } from "../../components/settings/settings-button";
import { SettingsPage } from "../../components/settings/settings-page";
import { SettingsRow } from "../../components/settings/settings-row";
import { SettingsSection } from "../../components/settings/settings-section";
import { Sheet } from "../../components/sheet";
import { tapFeedback } from "../../lib/haptics";
import { demoAgents, demoModels } from "../../lib/mock";
import { palette, radius, space } from "../../lib/theme";
import type { Agent } from "../../lib/types";

const tools = ["read/write", "read-only", "sandbox"] as const;

export default function AgentsScreen() {
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
    <>
      <SettingsPage
        right={
          <Pressable
            accessibilityLabel="New agent"
            accessibilityRole="button"
            hitSlop={8}
            onPress={openNew}
            style={({ pressed }) => [styles.circle, pressed && styles.pressed]}
          >
            <Ionicons color={palette.text} name="add" size={22} />
          </Pressable>
        }
        title="Agents"
      >
        <SettingsSection
          footer="Each agent runs on the gateway with its own model and tool access."
          title="On this workspace"
        >
          {agents.map((agent) => (
            <SettingsRow
              description={`${agent.model} · ${agent.tools}${
                agent.status === "paused" ? " · Paused" : ""
              }`}
              icon={
                agent.tools === "sandbox"
                  ? "code-slash-outline"
                  : "reader-outline"
              }
              key={agent.id}
              label={agent.name}
              onPress={() => openEdit(agent)}
            />
          ))}
        </SettingsSection>

        <View style={styles.action}>
          <SettingsButton label="New agent" onPress={openNew} />
        </View>
      </SettingsPage>

      <Sheet onClose={close} visible={sheetOpen}>
        <Text style={styles.sheetTitle}>
          {editing ? "Edit agent" : "New agent"}
        </Text>

        <Text style={styles.fieldLabel}>Name</Text>
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

        <Text style={styles.fieldLabel}>Summary</Text>
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

        <Text style={styles.fieldLabel}>Model</Text>
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

        <Text style={styles.fieldLabel}>Tools</Text>
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

        <View style={styles.sheetAction}>
          <SettingsButton
            disabled={!draft.name.trim()}
            label={editing ? "Save agent" : "Add agent"}
            onPress={save}
          />
        </View>
      </Sheet>
    </>
  );
}

const styles = StyleSheet.create({
  circle: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderRadius: radius.pill,
    height: 38,
    justifyContent: "center",
    width: 38,
  },
  action: {
    marginTop: -space.sm,
  },
  sheetTitle: {
    color: palette.text,
    fontSize: 22,
    fontWeight: "600",
    lineHeight: 28,
    marginBottom: space.lg,
  },
  fieldLabel: {
    color: palette.muted,
    fontSize: 13,
    fontWeight: "500",
    lineHeight: 18,
    marginBottom: space.xs,
    marginTop: space.md,
  },
  input: {
    backgroundColor: palette.bg,
    borderRadius: radius.md,
    color: palette.text,
    fontSize: 15,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingHorizontal: 12,
    paddingVertical: 12,
  },
  summaryInput: {
    minHeight: 76,
  },
  chips: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  chip: {
    backgroundColor: palette.bg,
    borderRadius: radius.pill,
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  chipActive: {
    backgroundColor: palette.accentSoft,
  },
  chipLabel: {
    color: palette.muted,
    fontSize: 13,
    lineHeight: 17,
  },
  chipLabelActive: {
    color: palette.accent,
    fontWeight: "600",
  },
  sheetAction: {
    marginBottom: space.sm,
    marginTop: space.xl,
  },
  pressed: {
    opacity: 0.7,
  },
});
