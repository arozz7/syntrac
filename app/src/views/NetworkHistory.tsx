import { useCallback, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { DestinationOrder, DestinationRowView, DestinationsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { DEFAULT_HISTORY_RANGE_SECS, HISTORY_RANGES } from '../components/historyRanges';
import { RangePicker } from '../components/RangePicker';
import { SortHeader } from '../components/SortHeader';
import { formatBytes, formatDateTime, formatDay } from '../format';
import { usePolled } from '../hooks/usePolled';
import { useSort, type SortControl } from '../hooks/useSort';
import { classLabel, nameLine, ownerLine } from '../networkLabels';
import type { Accessors } from '../sorting';
import { NetworkDialog, type NetworkFocus } from './NetworkDialog';
import { NewDestinations } from './NewDestinations';
import { OwnerDataBar } from './OwnerDataBar';
import { ProgramLink } from './ProgramLink';
import { TopOwners } from './TopOwners';

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

const SORTABLE: Accessors<DestinationRowView> = {
  destination: (row) => row.addr,
  received: (row) => row.bytesReceived,
  sent: (row) => row.bytesSent,
  flows: (row) => row.flows,
  programs: (row) => row.programs,
  firstSeen: (row) => row.firstSeenMs,
  lastSeen: (row) => row.lastSeenMs,
};

function DestinationsTable({
  view,
  sort,
  onSelect,
  onOpenProgram,
}: {
  view: DestinationsView;
  sort: SortControl;
  onSelect: (addr: string) => void;
  onOpenProgram: OpenProgram;
}) {
  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="destination" label="Destination" />
          <SortHeader sort={sort} column="received" label="Received" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="sent" label="Sent" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="flows" label="Flows" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="programs" label="Programs" firstDirection="desc" />
          <SortHeader sort={sort} column="firstSeen" label="First seen" firstDirection="desc" />
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
  const sort = useSort();
  // Moves when new owner data has been downloaded, so the tables are read again and show the owners.
  const [ownerVersion, setOwnerVersion] = useState(0);
  const [nowMs] = useState(() => Date.now());
  const ownerDataArrived = useCallback(() => setOwnerVersion((version) => version + 1), []);
  const openDestination = (addr: string) => setFocus({ kind: 'destination', addr });
  const openProgram: OpenProgram = (program, fromAddr) => setFocus({ kind: 'program', ...program, fromAddr });
  const result = usePolled(
    () => api.topDestinations(rangeSecs, order, undefined, allAddresses),
    REFRESH_MS,
    `destinations:${rangeSecs}:${order}:${allAddresses}:${ownerVersion}`,
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

      <OwnerDataBar nowMs={nowMs} onDownloaded={ownerDataArrived} />

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
                <DestinationsTable view={data} sort={sort} onSelect={openDestination} onOpenProgram={openProgram} />
              )}
            </>
          )}
        </DataGate>
      </section>

      <TopOwners
        rangeSecs={rangeSecs}
        allAddresses={allAddresses}
        refreshKey={ownerVersion}
        onSelect={openDestination}
      />

      <NewDestinations
        allAddresses={allAddresses}
        refreshKey={ownerVersion}
        onSelect={openDestination}
        onOpenProgram={openProgram}
      />

      {focus !== null && (
        <NetworkDialog focus={focus} rangeSecs={rangeSecs} onChange={setFocus} onClose={() => setFocus(null)} />
      )}

      <p className="muted">
        A name under an address is the name a program asked for when it reached that address. It is not who owns the
        address: large services answer thousands of names from one address, and one name from many addresses, so read it
        as a clue. An address with no name was reached by address, or before names were kept. A network owner is the
        organisation that runs the range of addresses it sits in, and the country that network is registered in: often a
        cloud or hosting company, not necessarily the service you reached, and not where this machine is. Traffic to
        this machine itself is left out, and addresses on this network, link-local and multicast addresses are left out
        unless included. Clicking a column heading sorts the destinations listed here (the biggest ones for the chosen
        order); a third click returns to that order.
      </p>
    </>
  );
}
