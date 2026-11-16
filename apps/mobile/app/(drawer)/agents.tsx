import { Ionicons } from "@expo/vector-icons";
import { useCallback, useEffect, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";

import { SettingsButton } from "../../components/settings/settings-button";
import { SettingsPage } from "../../components/settings/settings-page";
import { SettingsRow } from "../../components/settings/settings-row";
import { SettingsSection } from "../../components/settings/settings-section";
import { Sheet } from "../../components/sheet";
import { relativeTime } from "@goble/common";
import { tapFeedback } from "../../lib/haptics";
import { demoAgents, demoModels } from "../../lib/mock";
import { useConnection } from "../../lib/remote/connection";
import {
  removeAgent,
  requestAgents,
  saveAgent,
  type RemoteAgent,
} from "../../lib/remote/entities";
import type { AgentSpec } from "../../lib/remote/protocol";
import { palette, radius, space } from "../../lib/theme";
import type { Agent } from "../../lib/types";

const tools = ["read/write", "read-only", "sandbox"] as const;

/** A draft of the fields the phone can edit, for either kind of agent. */
interface Draft {
  name: string;
  summary: string;
  prompt: string;
  model: string;
  tools: string;
}

function emptyDraft(model: string): Draft {
  return { model, name: "", prompt: "", summary: "", tools: tools[0] };
}

/**
 * The agents on this workspace.
 *
 * Behind a gateway the list is the worker's own store, read with
 * `query_entities` and written with `update_agent`, so an agent made or edited
 * here is the one the worker runs. Without a gateway the page keeps the demo
 * workspace's local list, so the layout stays reviewable.
 */
export default function AgentsScreen() {
  const remote = useConnection();
  const live = remote.status === "live";
  const models = demoModels();
  const defaultModel = models[0]?.name ?? "GPT-4o";

  const [agents, setAgents] = useState<Agent[]>(() => demoAgents());
  const [remoteAgents, setRemoteAgents] = useState<RemoteAgent[]>([]);
  const [loading, setLoading] = useState(false);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const [sheetOpen, setSheetOpen] = useState(false);
  const [editing, setEditing] = useState<Agent | null>(null);
  const [editingRemote, setEditingRemote] = useState<RemoteAgent | null>(null);
  const [draft, setDraft] = useState<Draft>(() => emptyDraft(defaultModel));
  const [confirmRemove, setConfirmRemove] = useState(false);

  useEffect(() => {
    if (!live) {
      setRemoteAgents([]);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    requestAgents()
      .then((items) => {
        if (cancelled) return;
        setRemoteAgents(items);
        setError(null);
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setError(
          reason instanceof Error ? reason.message : "Could not read agents",
        );
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [live, reload]);

  const openNew = useCallback(() => {
    tapFeedback();
    setEditing(null);
    setEditingRemote(null);
    setConfirmRemove(false);
    setDraft(emptyDraft(defaultModel));
    setSheetOpen(true);
  }, [defaultModel]);

  function openEdit(agent: Agent) {
    tapFeedback();
    setEditing(agent);
    setEditingRemote(null);
    setConfirmRemove(false);
    setDraft({
      model: agent.model,
      name: agent.name,
      prompt: "",
      summary: agent.summary,
      tools: agent.tools,
    });
    setSheetOpen(true);
  }

  function openRemote(agent: RemoteAgent) {
    tapFeedback();
    const spec = agent.spec;
    setEditing(null);
    setEditingRemote(agent);
    setConfirmRemove(false);
    setDraft({
      model: defaultModel,
      name: agent.name,
      prompt: typeof spec.prompt === "string" ? spec.prompt : "",
      summary: typeof spec.description === "string" ? spec.description : "",
      tools:
        Array.isArray(spec.tools) && typeof spec.tools[0] === "string"
          ? spec.tools[0]
          : tools[0],
    });
    setSheetOpen(true);
  }

  function close() {
    setSheetOpen(false);
    setEditing(null);
    setEditingRemote(null);
    setConfirmRemove(false);
  }

  /** Write a draft to the worker, keeping the fields the phone does not edit. */
  function saveRemote() {
    const name = draft.name.trim();
    if (!name) return;
    tapFeedback();
    const now = new Date().toISOString();
    const base = editingRemote
      ? (editingRemote.spec as unknown as AgentSpec)
      : ({
          created_at: now,
          description: "",
          id: `agent-${Date.now().toString(36)}`,
          mcp_ids: [],
          name,
          prompt: "",
          tools: [],
          triggers: ["Manual"],
          updated_at: now,
        } satisfies AgentSpec);
    saveAgent({
      ...base,
      description: draft.summary.trim(),
      id: editingRemote?.id ?? base.id,
      name,
      prompt: draft.prompt,
      tools: [draft.tools],
      updated_at: now,
    });
    close();
    setReload((value) => value + 1);
  }

  function saveLocal() {
    const name = draft.name.trim();
    if (!name) return;
    tapFeedback();
    const summary = draft.summary.trim() || "Custom agent for this workspace.";
    setAgents((current) =>
      editing
        ? current.map((agent) =>
            agent.id === editing.id
              ? { ...agent, model: draft.model, name, summary, tools: draft.tools }
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

  function save() {
    if (live) saveRemote();
    else saveLocal();
  }

  function remove() {
    if (!editingRemote) return;
    if (!confirmRemove) {
      tapFeedback();
      setConfirmRemove(true);
      return;
    }
    tapFeedback();
    removeAgent(editingRemote.id);
    close();
    setReload((value) => value + 1);
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
        {live ? (
          <SettingsSection
            footer="Agents live on the worker. An edit made here is written there, so the next run uses it."
            title="On the worker"
          >
            {loading && remoteAgents.length === 0 ? (
              <View style={styles.loading}>
                <ActivityIndicator color={palette.muted} size="small" />
              </View>
            ) : null}
            {error ? (
              <SettingsRow
                chevron={false}
                description={error}
                detail="Retry"
                icon="alert-circle-outline"
                label="Could not read agents"
                onPress={() => setReload((value) => value + 1)}
              />
            ) : null}
            {!loading && !error && remoteAgents.length === 0 ? (
              <SettingsRow
                chevron={false}
                description="An agent made here is stored on the worker and runs from it."
                icon="people-outline"
                label="No agents yet"
              />
            ) : null}
            {remoteAgents.map((agent) => {
              const spec = agent.spec;
              const sandbox =
                Array.isArray(spec.tools) && spec.tools.includes("sandbox");
              const description =
                typeof spec.description === "string" && spec.description
                  ? spec.description
                  : "No description";
              return (
                <SettingsRow
                  description={`${description} · edited ${relativeTime(
                    agent.updated_at,
                  )}`}
                  icon={sandbox ? "code-slash-outline" : "reader-outline"}
                  key={agent.id}
                  label={agent.name}
                  onPress={() => openRemote(agent)}
                />
              );
            })}
          </SettingsSection>
        ) : (
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
        )}

        <View style={styles.action}>
          <SettingsButton label="New agent" onPress={openNew} />
        </View>
      </SettingsPage>

      <Sheet onClose={close} visible={sheetOpen}>
        <Text style={styles.sheetTitle}>
          {editing || editingRemote ? "Edit agent" : "New agent"}
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

        {live ? (
          <>
            <Text style={styles.fieldLabel}>Instructions</Text>
            <TextInput
              accessibilityLabel="Agent instructions"
              multiline
              onChangeText={(prompt) =>
                setDraft((current) => ({ ...current, prompt }))
              }
              placeholder="What the agent is told before every run"
              placeholderTextColor={palette.subtle}
              selectionColor={palette.accent}
              style={[styles.input, styles.promptInput]}
              textAlignVertical="top"
              value={draft.prompt}
            />
          </>
        ) : (
          <>
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
                      style={[
                        styles.chipLabel,
                        active && styles.chipLabelActive,
                      ]}
                    >
                      {model.name}
                    </Text>
                  </Pressable>
                );
              })}
            </View>
          </>
        )}

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
            label={editing || editingRemote ? "Save agent" : "Add agent"}
            onPress={save}
          />
        </View>

        {editingRemote ? (
          <Pressable
            accessibilityLabel="Remove agent"
            accessibilityRole="button"
            onPress={remove}
            style={({ pressed }) => [
              styles.remove,
              pressed && styles.pressed,
            ]}
          >
            <Text style={styles.removeLabel}>
              {confirmRemove ? "Tap again to remove" : "Remove agent"}
            </Text>
          </Pressable>
        ) : null}
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
  loading: {
    alignItems: "center",
    paddingVertical: space.lg,
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
  promptInput: {
    minHeight: 104,
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
  remove: {
    alignItems: "center",
    paddingVertical: space.md,
  },
  removeLabel: {
    color: palette.error,
    fontSize: 14,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.7,
  },
});
