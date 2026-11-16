import { useEffect, useState } from "react";

import {
  requestAgents,
  requestScheduledTasks,
  useConnection,
  type RemoteAgent,
  type RemoteSchedule,
} from "@goble/common";

/** The worker's stored agents and schedules, as the overview draws them. */
export interface WorkerEntities {
  agents: RemoteAgent[];
  schedules: RemoteSchedule[];
  agentsError: string | null;
  schedulesError: string | null;
}

const EMPTY: WorkerEntities = {
  agents: [],
  schedules: [],
  agentsError: null,
  schedulesError: null,
};

function describe(error: unknown): string {
  return error instanceof Error ? error.message : "The worker did not answer";
}

/**
 * Read the worker's agents and scheduled tasks while the relay is live.
 *
 * The trigger is the connection status, as with the conversation list: a socket
 * that goes live (or comes back) reads once, and nothing polls on a timer. Each
 * type settles on its own, so one failure keeps that half's last good list and
 * reports a line instead of blanking the card or rejecting the whole read.
 */
export function useWorkerEntities(): WorkerEntities {
  const remote = useConnection();
  const [entities, setEntities] = useState<WorkerEntities>(EMPTY);

  useEffect(() => {
    if (remote.status !== "live") return;
    let cancelled = false;

    void requestAgents()
      .then((agents) => {
        if (cancelled) return;
        setEntities((current) => ({ ...current, agents, agentsError: null }));
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setEntities((current) => ({
          ...current,
          agentsError: describe(error),
        }));
      });

    void requestScheduledTasks()
      .then((schedules) => {
        if (cancelled) return;
        setEntities((current) => ({
          ...current,
          schedules,
          schedulesError: null,
        }));
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setEntities((current) => ({
          ...current,
          schedulesError: describe(error),
        }));
      });

    return () => {
      cancelled = true;
    };
  }, [remote.status]);

  return entities;
}
