import { useEffect, useState } from "react";

import {
  requestExecutions,
  requestSubagents,
  requestWorkflows,
  useConnection,
  type RemoteExecution,
  type RemoteSubagent,
  type RemoteWorkflow,
} from "@goble/common";

/** The worker's stored subagents, workflows and runs, as the overview draws them. */
export interface WorkerWork {
  subagents: RemoteSubagent[];
  workflows: RemoteWorkflow[];
  executions: RemoteExecution[];
  subagentsError: string | null;
  workflowsError: string | null;
  executionsError: string | null;
}

const EMPTY: WorkerWork = {
  subagents: [],
  workflows: [],
  executions: [],
  subagentsError: null,
  workflowsError: null,
  executionsError: null,
};

function describe(error: unknown): string {
  return error instanceof Error ? error.message : "The worker did not answer";
}

/**
 * Read the worker's subagents, workflows and runs while the relay is live.
 *
 * Like the agents and schedules read, the connection status is the trigger: a
 * socket that goes live (or comes back) reads once, and nothing polls. The
 * three kinds settle independently, so a failed read keeps that card's last
 * good list and reports a line instead of blanking the other two.
 */
export function useWorkerWork(): WorkerWork {
  const remote = useConnection();
  const [work, setWork] = useState<WorkerWork>(EMPTY);

  useEffect(() => {
    if (remote.status !== "live") return;
    let cancelled = false;

    void requestSubagents()
      .then((subagents) => {
        if (cancelled) return;
        setWork((current) => ({
          ...current,
          subagents: [...subagents].sort((a, b) =>
            b.updated_at.localeCompare(a.updated_at),
          ),
          subagentsError: null,
        }));
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setWork((current) => ({
          ...current,
          subagentsError: describe(error),
        }));
      });

    void requestWorkflows()
      .then((workflows) => {
        if (cancelled) return;
        setWork((current) => ({
          ...current,
          workflows: [...workflows].sort((a, b) =>
            b.updated_at.localeCompare(a.updated_at),
          ),
          workflowsError: null,
        }));
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setWork((current) => ({
          ...current,
          workflowsError: describe(error),
        }));
      });

    void requestExecutions()
      .then((executions) => {
        if (cancelled) return;
        setWork((current) => ({
          ...current,
          executions: [...executions].sort((a, b) =>
            b.started_at.localeCompare(a.started_at),
          ),
          executionsError: null,
        }));
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setWork((current) => ({
          ...current,
          executionsError: describe(error),
        }));
      });

    return () => {
      cancelled = true;
    };
  }, [remote.status]);

  return work;
}
