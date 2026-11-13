// Formatting for telemetry values. Pure, so it is unit-tested.

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 B';
  }
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${Math.round(value)} B` : `${value.toFixed(1)} ${BYTE_UNITS[unit]}`;
}

/** A share of the whole machine. Tiny non-zero values read "<0.1%" rather than a misleading "0.0%". */
export function formatPercent(percent: number, digits = 1): string {
  if (!Number.isFinite(percent) || percent <= 0) {
    return `${(0).toFixed(digits)}%`;
  }
  const smallest = 10 ** -digits;
  return percent < smallest / 2 ? `<${smallest.toFixed(digits)}%` : `${percent.toFixed(digits)}%`;
}

export function formatDuration(totalSecs: number): string {
  const secs = Math.max(0, Math.floor(totalSecs));
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  const seconds = secs % 60;
  if (days > 0) {
    return `${days} d ${hours} h`;
  }
  if (hours > 0) {
    return `${hours} h ${minutes} min`;
  }
  if (minutes > 0) {
    return `${minutes} min ${seconds} s`;
  }
  return `${seconds} s`;
}

/** How long ago something happened, for "data freshness". */
export function formatAge(secs: number): string {
  const age = Math.max(0, Math.floor(secs));
  if (age < 5) {
    return 'just now';
  }
  if (age < 60) {
    return `${age} s ago`;
  }
  if (age < 3600) {
    return `${Math.floor(age / 60)} min ago`;
  }
  if (age < 86400) {
    return `${Math.floor(age / 3600)} h ago`;
  }
  return `${Math.floor(age / 86400)} d ago`;
}

const pad = (n: number): string => String(n).padStart(2, '0');

/** Local time, `HH:MM:SS`. */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** Local date, `YYYY-MM-DD`. */
export function formatDay(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Local date and time, `YYYY-MM-DD HH:MM`. */
export function formatDateTime(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
