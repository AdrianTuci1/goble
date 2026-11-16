import { useRouter } from "expo-router";
import { useEffect, useState } from "react";

import {
  SettingsPage,
  SettingsRow,
  SettingsSection,
  plural,
  relativeTime,
  requestSubagents,
  useConnection,
  type RemoteSubagent,
} from "@goble/common";
import { tapFeedback } from "../../lib/haptics";

/** Collapse a stored message onto the one line a row preview has room for. */
function preview(text: string | null): string {
  if (!text) return "No messages yet";
  const line = text.replace(/\s+/g, " ").trim();
  if (!line) return "No messages yet";
  return line.length > 120 ? `${line.slice(0, 119)}…` : line;
}

/**
 * The child chats the worker stores.
 *
 * A subagent is a conversation the agent spawns for a side task, so it lives
 * outside the top-level chat list. Behind a gateway this reads every one it
 * stores; without one there is nothing to invent, so the page says so.
 */
export default function SubagentsScreen() {
  const router = useRouter();
  const remote = useConnection();
  const live = remote.status === "live";
  const [subagents, setSubagents] = useState<RemoteSubagent[]>([]);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!live) {
      setSubagents([]);
      setError(null);
      return;
    }
    let cancelled = false;
    requestSubagents()
      .then((items) => {
        if (cancelled) return;
        setSubagents(
          [...items].sort((a, b) => b.updated_at.localeCompare(a.updated_at)),
        );
        setError(null);
      })
      .catch((reason: unknown) => {
        if (cancelled) return;
        setError(
          reason instanceof Error ? reason.message : "Could not read subagents",
        );
      });
    return () => {
      cancelled = true;
    };
  }, [live, reload]);

  function open(id: string) {
    tapFeedback();
    router.push(`/session/${id}`);
  }

  return (
    <SettingsPage title="Subagents">
      <SettingsSection
        footer="A subagent keeps its own transcript, separate from the conversation that spawned it."
        title="Subagents"
      >
        {error ? (
          <SettingsRow
            chevron={false}
            description={error}
            detail="Retry"
            icon="alert-circle-outline"
            label="Could not read subagents"
            onPress={() => setReload((value) => value + 1)}
          />
        ) : null}
        {!error && !live ? (
          <SettingsRow
            chevron={false}
            description="Subagents appear here when the agent spawns one. Pair a gateway in Settings to read the worker's subagents."
            icon="cloud-offline-outline"
            label="No gateway connected"
          />
        ) : null}
        {!error && live && subagents.length === 0 ? (
          <SettingsRow
            chevron={false}
            description="Subagents appear here when the agent spawns one."
            icon="git-branch-outline"
            label="No subagents yet"
          />
        ) : null}
        {live
          ? subagents.map((subagent) => (
              <SettingsRow
                description={`${
                  subagent.parent_title ?? "Unknown parent"
                } · ${plural(
                  subagent.message_count,
                  "message",
                  "messages",
                )} · ${preview(subagent.last_message)}`}
                detail={relativeTime(
                  subagent.last_message_at ?? subagent.updated_at,
                )}
                icon="git-branch-outline"
                key={subagent.id}
                label={subagent.title}
                onPress={() => open(subagent.id)}
              />
            ))
          : null}
      </SettingsSection>
    </SettingsPage>
  );
}
