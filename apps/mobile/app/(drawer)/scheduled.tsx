import { SettingsPage } from "../../components/settings/settings-page";
import { SettingsRow } from "../../components/settings/settings-row";
import { SettingsSection } from "../../components/settings/settings-section";
import { naturalSchedule, relativeTime, until } from "../../lib/format";
import { demoSchedules } from "../../lib/mock";

export default function ScheduledScreen() {
  const schedules = demoSchedules();

  return (
    <SettingsPage title="Scheduled">
      <SettingsSection
        footer="Workflows the gateway runs without a session. Paused schedules stay listed here until you resume them."
        title="Schedules"
      >
        {schedules.map((task) => {
          const active = task.status === "active";
          const last = task.lastRun
            ? ` · last ${relativeTime(task.lastRun)}`
            : "";
          return (
            <SettingsRow
              chevron={false}
              description={`${naturalSchedule(task.schedule)} · next ${until(
                task.nextRun,
              )}${last}`}
              detail={active ? "Active" : "Paused"}
              detailTone={active ? "success" : "warning"}
              icon="calendar-outline"
              key={task.id}
              label={task.name}
            />
          );
        })}
      </SettingsSection>
    </SettingsPage>
  );
}
