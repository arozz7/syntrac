import type { HealthState, HealthView } from '../api/types';
import { formatAge, formatDuration } from '../format';

const LABELS: Record<HealthState, string> = {
  running: 'Running',
  stale: 'Stale',
  offline: 'Offline',
  unknown: 'Unknown',
};

// A shape per state as well as a colour and a word: severity is never conveyed by colour alone.
const GLYPHS: Record<HealthState, string> = { running: '●', stale: '▲', offline: '✖', unknown: '?' };

/** The collector's health and the freshness of the data, visible on every screen. */
export function StatusBanner({ health }: { health: HealthView | undefined }) {
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
  if (health.etwDropped !== null && health.etwDropped > 0) {
    details.push(`${health.etwDropped} events dropped`);
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
