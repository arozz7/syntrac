import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { NewDestinationsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { SortHeader } from '../components/SortHeader';
import { formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { useSort, type SortControl } from '../hooks/useSort';
import type { Accessors } from '../sorting';
import { classLabel, nameLine, ownerLine } from '../networkLabels';
import { ProgramLink } from './ProgramLink';

const REFRESH_MS = 60_000;
const DAY_SECS = 86_400;

const WINDOWS: Array<{ label: string; secs: number }> = [
  { label: 'Last day', secs: DAY_SECS },
  { label: 'Last 7 days', secs: 7 * DAY_SECS },
  { label: 'Last 30 days', secs: 30 * DAY_SECS },
];
const DEFAULT_WINDOW_SECS = 7 * DAY_SECS;

type OpenProgram = (program: { name: string; path: string }, fromAddr: string) => void;

const SORTABLE: Accessors<NewDestinationsView['destinations'][number]> = {
  destination: (row) => row.addr,
  firstSeen: (row) => row.firstSeenMs,
  firstProgram: (row) => row.firstProgram,
  lastSeen: (row) => row.lastSeenMs,
};

function NewTable({
  view,
  sort,
  onSelect,
  onOpenProgram,
}: {
  view: NewDestinationsView;
  sort: SortControl;
  onSelect: (addr: string) => void;
  onOpenProgram: OpenProgram;
}) {
  if (view.destinations.length === 0) {
    return <p className="muted">Nothing new in this period.</p>;
  }
  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="destination" label="Destination" />
          <SortHeader sort={sort} column="firstSeen" label="First seen" firstDirection="desc" />
          <SortHeader sort={sort} column="firstProgram" label="First talked to by" />
          <SortHeader sort={sort} column="lastSeen" label="Last seen" firstDirection="desc" />
        </tr>
      </thead>
      <tbody>
        {sort.apply(view.destinations, SORTABLE).map((row) => {
          const kind = classLabel(row.class);
          return (
            <tr key={row.addr}>
              <td>
                <button type="button" className="linklike" onClick={() => onSelect(row.addr)}>
                  {row.addr}
                </button>{' '}
                {kind !== null && <span className="badge badge--quiet">{kind}</span>}
                <div className="muted">{nameLine(row.name, row.otherNames)}</div>
                {row.owner !== null && <div className="muted">{ownerLine(row.owner)}</div>}
              </td>
              <td>{formatDateTime(row.firstSeenMs)}</td>
              <td>
                {row.firstProgram !== null && row.firstProgramPath !== null ? (
                  <ProgramLink
                    name={row.firstProgram}
                    path={row.firstProgramPath}
                    onOpen={(program) => onOpenProgram(program, row.addr)}
                  />
                ) : (
                  (row.firstProgram ?? 'Unknown program')
                )}
              </td>
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
  refreshKey = 0,
  onSelect,
  onOpenProgram,
}: {
  allAddresses: boolean;
  /** Changes when the screen should read again (for example after owner data was downloaded). */
  refreshKey?: number;
  onSelect: (addr: string) => void;
  onOpenProgram: OpenProgram;
}) {
  const api = useApi();
  const [windowSecs, setWindowSecs] = useState(DEFAULT_WINDOW_SECS);
  const sort = useSort();
  const result = usePolled(
    () => api.newDestinations(windowSecs, undefined, allAddresses),
    REFRESH_MS,
    `new-destinations:${windowSecs}:${allAddresses}:${refreshKey}`,
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
            <NewTable view={data} sort={sort} onSelect={onSelect} onOpenProgram={onOpenProgram} />
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
