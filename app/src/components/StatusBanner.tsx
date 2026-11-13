import type { HealthState, HealthView } from '../api/types';
import { formatAge, formatBytes, formatDuration } from '../format';

/** A few quiet seconds from ETW are normal; only longer silences are worth a line in the banner. */
const ETW_SILENCE_WORTH_SHOWING_SECS = 60;

const LABELS: Record<HealthState, string> = {
  running: 'Running',
  stale: 'Stale',
  degraded: 'Degraded',
  offline: 'Offline',
  unknown: 'Unknown',
};

// A shape per state as well as a colour and a word: severity is never conveyed by colour alone.
const GLYPHS: Record<HealthState, string> = { running: '●', stale: '▲', degraded: '◆', offline: '✖', unknown: '?' };

/**
 * The collector's health and the freshness of the data, visible on every screen. `nowMs` is the moment `health` was fetched; it
 * is passed in (never read here) so the component stays pure, and the "how far back" ages appear only when it is given.
 */
export function StatusBanner({ health, nowMs }: { health: HealthView | undefined; nowMs?: number }) {
  if (!health) {
    return (
      <div className="status status--checking" role="status" data-state="checking">
        <span className="status__label">Checking the collector…</span>
      </div>
    );
  }

  const details: string[] = [];
  if (health.dataAgeSecs !== null) {
    details.push(`Data updated ${formatAge(health.dataAgeSecs)}`);
  }
  if (health.uptimeSecs !== null) {
    details.push(`Up ${formatDuration(health.uptimeSecs)}`);
  }
  if (health.etwSilentSecs !== null && health.etwSilentSecs >= ETW_SILENCE_WORTH_SHOWING_SECS) {
    details.push(`ETW silent ${formatDuration(health.etwSilentSecs)}`);
  }
  if (health.etwDropped !== null && health.etwDropped > 0) {
    details.push(`${health.etwDropped} events dropped`);
  }
  if (health.storage) {
    const ageSecs = (thenMs: number, now: number) => Math.max(0, (now - thenMs) / 1000);
    details.push(`Database ${formatBytes(health.storage.databaseLiveBytes)}`);
    details.push(`Drive free ${formatBytes(health.storage.driveFreeBytes)}`);
    if (nowMs !== undefined && health.storage.oldestDetailMs !== null) {
      details.push(`Detail back ${formatAge(ageSecs(health.storage.oldestDetailMs, nowMs))}`);
    }
    if (nowMs !== undefined && health.storage.oldestRollupMs !== null) {
      details.push(`Summaries back ${formatAge(ageSecs(health.storage.oldestRollupMs, nowMs))}`);
    }
    if (health.storage.trimmedHoursTotal > 0) {
      details.push(`${health.storage.trimmedHoursTotal} h trimmed early`);
    }
  }

  return (
    <div className={`status status--${health.state}`} role="status" data-state={health.state}>
      <span className="status__label">
        <span aria-hidden="true">{GLYPHS[health.state]}</span> {LABELS[health.state]}
      </span>
      <span className="status__message">{health.message}</span>
      {details.length > 0 && <span className="status__details muted">{details.join(' · ')}</span>}
    </div>
  );
}
