import { useApi } from '../api/ApiContext';
import type { ProgramDestinationRowView, ProgramDestinationsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { SortHeader } from '../components/SortHeader';
import { formatBytes, formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { useSort, type SortControl } from '../hooks/useSort';
import { classLabel, nameLine, ownerLine } from '../networkLabels';
import type { Accessors } from '../sorting';

const REFRESH_MS = 60_000;

const SORTABLE: Accessors<ProgramDestinationRowView> = {
  destination: (row) => row.addr,
  received: (row) => row.bytesReceived,
  sent: (row) => row.bytesSent,
  flows: (row) => row.flows,
  firstSeen: (row) => row.firstSeenMs,
  lastSeen: (row) => row.lastSeenMs,
};

function DestinationsTable({
  view,
  sort,
  onOpenDestination,
}: {
  view: ProgramDestinationsView;
  sort: SortControl;
  onOpenDestination: ((addr: string) => void) | undefined;
}) {
  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="destination" label="Destination" />
          <SortHeader sort={sort} column="received" label="Received" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="sent" label="Sent" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="flows" label="Flows" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="firstSeen" label="First reached" firstDirection="desc" />
          <SortHeader sort={sort} column="lastSeen" label="Last seen" firstDirection="desc" />
        </tr>
      </thead>
      <tbody>
        {sort.apply(view.destinations, SORTABLE).map((row) => {
          const kind = classLabel(row.class);
          return (
            <tr key={row.addr}>
              <td>
                {onOpenDestination === undefined ? (
                  row.addr
                ) : (
                  <button type="button" className="linklike" onClick={() => onOpenDestination(row.addr)}>
                    {row.addr}
                  </button>
                )}{' '}
                {kind !== null && <span className="badge badge--quiet">{kind}</span>}
                {row.isNew && <span className="badge">New</span>}
                <div className="muted">{nameLine(row.name, row.otherNames)}</div>
                <div className="muted">{row.owner === null ? 'No owner found' : ownerLine(row.owner)}</div>
              </td>
              <td className="num">{formatBytes(row.bytesReceived)}</td>
              <td className="num">{formatBytes(row.bytesSent)}</td>
              <td className="num">{row.flows}</td>
              <td>{row.firstSeenMs === null ? '—' : formatDateTime(row.firstSeenMs)}</td>
              <td>{formatDateTime(row.lastSeenMs)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/**
 * Where one program connected over the range: its own destinations, biggest first. "New" means this program first reached the
 * address within the range (after the first day of history, which counts as the baseline).
 */
export function ProgramDestinations({
  path,
  rangeSecs,
  onOpenDestination,
}: {
  path: string;
  rangeSecs: number;
  /** Makes each address a link that opens it. Left out where the page has nowhere to open it. */
  onOpenDestination?: (addr: string) => void;
}) {
  const api = useApi();
  const sort = useSort();
  const result = usePolled(
    () => api.programDestinations(rangeSecs, path, undefined, undefined),
    REFRESH_MS,
    `program-destinations:${path}:${rangeSecs}`,
  );
  return (
    <>
      <h3>Where it connected</h3>
      <DataGate result={result}>
        {(data) =>
          data.destinations.length === 0 ? (
            <p className="muted">
              No destinations on the internet in this period. Traffic to this machine itself and to addresses on this
              network is left out.
            </p>
          ) : (
            <>
              <DestinationsTable view={data} sort={sort} onOpenDestination={onOpenDestination} />
              {data.totalDestinations > data.destinations.length && (
                <p className="muted">
                  Showing the biggest {data.destinations.length} of {data.totalDestinations} destinations.
                </p>
              )}
            </>
          )
        }
      </DataGate>
    </>
  );
}
