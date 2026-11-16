import { useRouter } from "expo-router";

import { ChoicePage } from "../../../components/settings/choice-page";
import { relativeTime } from "@goble/common";
import { connection, useConnection } from "../../../lib/remote/connection";

export default function WorkersScreen() {
  const router = useRouter();
  const { workers, workerId } = useConnection();

  return (
    <ChoicePage
      footer="A worker is one machine or sandbox attached to the gateway. The phone relays every turn and every terminal pane to the worker you pick."
      groups={[
        {
          options: workers.map((worker) => ({
            description:
              worker.id === workerId
                ? "This phone relays here · connected " +
                  relativeTime(worker.connected_at)
                : "Connected " + relativeTime(worker.connected_at),
            detail: relativeTime(worker.last_seen),
            id: worker.id,
            label: worker.id,
          })),
          title: "Connected workers",
        },
      ]}
      onSelect={(id) => {
        void connection.selectWorker(id);
        router.back();
      }}
      placeholder="Search workers"
      title="Worker"
      value={workerId}
    />
  );
}
