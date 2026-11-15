import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { NewDestinationsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { classLabel, nameLine } from '../networkLabels';

const REFRESH_MS = 60_000;
const DAY_SECS = 86_400;

const WINDOWS: Array<{ label: string; secs: number }> = [
  { label: 'Last day', secs: DAY_SECS },
  { label: 'Last 7 days', secs: 7 * DAY_SECS },
  { label: 'Last 30 days', secs: 30 * DAY_SECS },
];
const DEFAULT_WINDOW_SECS = 7 * DAY_SECS;

function NewTable({ view, onSelect }: { view: NewDestinationsView; onSelect: (addr: string) => void }) {
  if (view.destinations.length === 0) {
    return <p className="muted">Nothing new in this period.</p>;
  }
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Destination</th>
          <th>First seen</th>
          <th>First talked to by</th>
          <th>Last seen</th>
        </tr>
      </thead>
      <tbody>
        {view.destinations.map((row) => {
          const kind = classLabel(row.class);
          return (
            <tr key={row.addr}>
              <td>
                <button type="button" className="linklike" onClick={() => onSelect(row.addr)}>
                  {row.addr}
                </button>{' '}
                {kind !== null && <span className="badge badge--quiet">{kind}</span>}
                <div className="muted">{nameLine(row.name, row.otherNames)}</div>
              </td>
              <td>{formatDateTime(row.firstSeenMs)}</td>
              <td>{row.firstProgram ?? 'Unknown program'}</td>
              <td>{formatDateTime(row.lastSeenMs)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Destinations that appeared for the first time recently: the beginning of "something is different". */
export function NewDestinations({
  allAddresses,
  onSelect,
}: {
  allAddresses: boolean;
  onSelect: (addr: string) => void;
}) {
  const api = useApi();
  const [windowSecs, setWindowSecs] = useState(DEFAULT_WINDOW_SECS);
  const result = usePolled(
    () => api.newDestinations(windowSecs, undefined, allAddresses),
    REFRESH_MS,
    `new-destinations:${windowSecs}:${allAddresses}`,
  );
  return (
    <section className="card card--wide">
      <div className="card__head">
        <h2>New destinations</h2>
        <div className="range" role="group" aria-label="New since">
          {WINDOWS.map((entry) => (
            <button
              key={entry.secs}
              type="button"
              className="range__button"
              aria-pressed={windowSecs === entry.secs}
              onClick={() => setWindowSecs(entry.secs)}
            >
              {entry.label}
            </button>
          ))}
        </div>
      </div>
      <p className="muted">Addresses first seen in this period, with the program that was first to talk to each.</p>
      <DataGate result={result}>
        {(data) => (
          <>
            <NewTable view={data} onSelect={onSelect} />
            {data.sinceMs !== null && data.sinceMs === data.baselineUntilMs && (
              <p className="muted">
                Everything first seen before {formatDateTime(data.baselineUntilMs)} counts as the baseline, not as new:
                on the first day of history every address is new.
              </p>
            )}
          </>
        )}
      </DataGate>
    </section>
  );
}
