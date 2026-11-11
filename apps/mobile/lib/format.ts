export function relativeTime(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days}d ago`;
  return new Date(then).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

export function until(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const minutes = Math.max(1, Math.round((then - now) / 60_000));
  if (minutes < 60) return `in ${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `in ${hours}h`;
  const days = Math.round(hours / 24);
  return `in ${days}d`;
}

export function duration(seconds?: number): string {
  if (seconds === undefined) return "";
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return rest ? `${minutes}m ${rest}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function greeting(date = new Date()): string {
  const hour = date.getHours();
  if (hour < 5) return "Working late";
  if (hour < 12) return "Good morning";
  if (hour < 18) return "Good afternoon";
  return "Good evening";
}

export function plural(count: number, singular: string, pluralForm: string): string {
  return `${count} ${count === 1 ? singular : pluralForm}`;
}

export function hostLabel(url: string | null): string {
  if (!url) return "Demo workspace";
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

const weekdays = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

/** Turn a cron expression or "Every N min" into a spoken frequency. */
export function naturalSchedule(schedule: string): string {
  const text = schedule.trim();
  const every = /^every\s+(\d+)\s*(m|min|mins|minute|minutes|h|hr|hrs|hour|hours)$/i.exec(
    text,
  );
  if (every) {
    const count = Number(every[1]);
    const unit = /^h/i.test(every[2]!)
      ? count === 1
        ? "hour"
        : "hours"
      : count === 1
        ? "minute"
        : "minutes";
    return `Every ${count} ${unit}`;
  }

  const parts = text.split(/\s+/);
  if (parts.length !== 5) return text;
  const [minute, hour, dayOfMonth, month, dayOfWeek] = parts;
  if (!minute || !hour || !dayOfMonth || !month || !dayOfWeek) return text;

  if (minute.startsWith("*/")) {
    const count = Number(minute.slice(2));
    return Number.isFinite(count)
      ? `Every ${count} ${count === 1 ? "minute" : "minutes"}`
      : text;
  }
  if (minute === "0" && hour.startsWith("*/")) {
    const count = Number(hour.slice(2));
    return Number.isFinite(count)
      ? `Every ${count} ${count === 1 ? "hour" : "hours"}`
      : text;
  }
  if (dayOfMonth !== "*" || month !== "*" || !/^\d+$/.test(hour)) return text;

  const time = `${hour.padStart(2, "0")}:${minute.padStart(2, "0")}`;
  if (dayOfWeek === "*") return `Every day at ${time}`;
  if (dayOfWeek === "1-5") return `Every weekday at ${time}`;
  if (dayOfWeek === "0,6" || dayOfWeek === "6,0") {
    return `Every weekend at ${time}`;
  }
  if (/^[0-6]$/.test(dayOfWeek)) {
    return `Every ${weekdays[Number(dayOfWeek)]} at ${time}`;
  }
  return text;
}
