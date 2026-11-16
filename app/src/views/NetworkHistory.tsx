import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { DestinationOrder, DestinationRowView, DestinationsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { DEFAULT_HISTORY_RANGE_SECS, HISTORY_RANGES } from '../components/historyRanges';
import { RangePicker } from '../components/RangePicker';
import { formatBytes, formatDateTime, formatDay } from '../format';
import { usePolled } from '../hooks/usePolled';
import { classLabel, nameLine } from '../networkLabels';
import { NetworkDialog, type NetworkFocus } from './NetworkDialog';
import { NewDestinations } from './NewDestinations';
import { ProgramLink } from './ProgramLink';

const REFRESH_MS = 60_000;

const ORDERS: Array<{ id: DestinationOrder; label: string }> = [
  { id: 'bytes', label: 'Bytes' },
  { id: 'flows', label: 'Flows' },
  { id: 'programs', label: 'Programs' },
];

type OpenProgram = (program: { name: string; path: string }, fromAddr: string) => void;

/** The biggest programs of a row, each a link to its history, and what is left over as one button that opens the address. */
function ProgramsCell({
  row,
  onSelect,
  onOpenProgram,
}: {
  row: DestinationRowView;
  onSelect: (addr: string) => void;
  onOpenProgram: OpenProgram;
}) {
  return (
    <td className="programs-cell">
      {row.topPrograms.map((program) =>
        program.path === null ? (
          <span key={program.name}>{program.name}</span>
        ) : (
          <span key={program.path}>
            <ProgramLink name={program.name} path={program.path} onOpen={(chosen) => onOpenProgram(chosen, row.addr)} />
          </span>
        ),
      )}
      {row.morePrograms > 0 && (
        <button type="button" className="linklike" onClick={() => onSelect(row.addr)}>
          +{row.morePrograms} more
        </button>
      )}
    </td>
  );
}

function DestinationsTable({
  view,
  onSelect,
  onOpenProgram,
}: {
  view: DestinationsView;
  onSelect: (addr: string) => void;
  onOpenProgram: OpenProgram;
}) {
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Destination</th>
          <th className="num">Received</th>
          <th className="num">Sent</th>
          <th className="num">Flows</th>
          <th>Programs</th>
          <th>First seen</th>
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
              <td className="num">{formatBytes(row.bytesReceived)}</td>
              <td className="num">{formatBytes(row.bytesSent)}</td>
              <td className="num">{row.flows}</td>
              <ProgramsCell row={row} onSelect={onSelect} onOpenProgram={onOpenProgram} />
              <td>{row.firstSeenMs === null ? '—' : formatDateTime(row.firstSeenMs)}</td>
              <td>{row.lastSeenMs === null ? '—' : formatDateTime(row.lastSeenMs)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Where the machine talks to over days and months, what is new, and one destination in detail, from the hourly summaries. */
export function NetworkHistory() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_HISTORY_RANGE_SECS);
  const [order, setOrder] = useState<DestinationOrder>('bytes');
  const [allAddresses, setAllAddresses] = useState(false);
  const [focus, setFocus] = useState<NetworkFocus | null>(null);
  const openDestination = (addr: string) => setFocus({ kind: 'destination', addr });
  const openProgram: OpenProgram = (program, fromAddr) => setFocus({ kind: 'program', ...program, fromAddr });
  const result = usePolled(
    () => api.topDestinations(rangeSecs, order, undefined, allAddresses),
    REFRESH_MS,
    `destinations:${rangeSecs}:${order}:${allAddresses}`,
  );

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} options={HISTORY_RANGES} />
        <div className="range" role="group" aria-label="Order by">
          {ORDERS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              className="range__button"
              aria-pressed={order === entry.id}
              onClick={() => setOrder(entry.id)}
            >
              {entry.label}
            </button>
          ))}
        </div>
        <div className="range" role="group" aria-label="Addresses">
          <button
            type="button"
            className="range__button"
            aria-pressed={allAddresses}
            onClick={() => setAllAddresses((current) => !current)}
          >
            Include this network and multicast
          </button>
        </div>
      </div>

      <section className="card card--wide">
        <h2>Top destinations</h2>
        <DataGate result={result}>
          {(data) => (
            <>
              {data.historyStartsMs !== null && data.historyStartsMs > data.fromMs && (
                <p className="muted">
                  History begins {formatDay(data.historyStartsMs)}. Earlier time was not collected.
                </p>
              )}
              {data.destinations.length === 0 ? (
                <p className="muted">No destinations to show for this period.</p>
              ) : (
                <DestinationsTable view={data} onSelect={openDestination} onOpenProgram={openProgram} />
              )}
            </>
          )}
        </DataGate>
      </section>

      <NewDestinations allAddresses={allAddresses} onSelect={openDestination} onOpenProgram={openProgram} />

      {focus !== null && (
        <NetworkDialog focus={focus} rangeSecs={rangeSecs} onChange={setFocus} onClose={() => setFocus(null)} />
      )}

      <p className="muted">
        A name under an address is the name a program asked for when it reached that address. It is not who owns the
        address: large services answer thousands of names from one address, and one name from many addresses, so read it
        as a clue. An address with no name was reached by address, or before names were kept. Traffic to this machine
        itself is left out, and addresses on this network, link-local and multicast addresses are left out unless
        included.
      </p>
    </>
  );
}
