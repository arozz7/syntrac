import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { OwnerRowView, OwnersView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { SortHeader } from '../components/SortHeader';
import { formatBytes, formatDay } from '../format';
import { usePolled } from '../hooks/usePolled';
import { useSort, type SortControl } from '../hooks/useSort';
import { ownerLine } from '../networkLabels';
import type { Accessors } from '../sorting';

const REFRESH_MS = 60_000;
const NO_OWNER = 'No owner found';

const SORTABLE: Accessors<OwnerRowView> = {
  owner: (row) => (row.owner === null ? null : row.owner.org || `AS${row.owner.asn}`),
  received: (row) => row.bytesReceived,
  sent: (row) => row.bytesSent,
  flows: (row) => row.flows,
  destinations: (row) => row.destinations,
};

function keyOf(row: OwnerRowView): string {
  return row.owner === null ? 'none' : `AS${row.owner.asn}`;
}

function OwnersTable({
  view,
  sort,
  onSelect,
}: {
  view: OwnersView;
  sort: SortControl;
  onSelect: (addr: string) => void;
}) {
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const toggle = (key: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (!next.delete(key)) {
        next.add(key);
      }
      return next;
    });

  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="owner" label="Owner" />
          <SortHeader sort={sort} column="received" label="Received" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="sent" label="Sent" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="flows" label="Flows" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="destinations" label="Destinations" firstDirection="desc" className="num" />
        </tr>
      </thead>
      <tbody>
        {sort.apply(view.owners, SORTABLE).flatMap((row) => {
          const key = keyOf(row);
          const expanded = open.has(key);
          const main = (
            <tr key={key}>
              <td>
                <button type="button" className="linklike" aria-expanded={expanded} onClick={() => toggle(key)}>
                  {row.owner === null ? NO_OWNER : ownerLine(row.owner)}
                </button>
              </td>
              <td className="num">{formatBytes(row.bytesReceived)}</td>
              <td className="num">{formatBytes(row.bytesSent)}</td>
              <td className="num">{row.flows}</td>
              <td className="num">{row.destinations}</td>
            </tr>
          );
          if (!expanded) {
            return [main];
          }
          const details = row.topDestinations.map((destination) => (
            <tr key={`${key}|${destination.addr}`} className="table__member">
              <td>
                <button type="button" className="linklike" onClick={() => onSelect(destination.addr)}>
                  {destination.addr}
                </button>{' '}
                {destination.name !== null && <span className="muted">{destination.name}</span>}
              </td>
              <td className="num">{formatBytes(destination.bytesReceived)}</td>
              <td className="num">{formatBytes(destination.bytesSent)}</td>
              <td colSpan={2} />
            </tr>
          ));
          if (row.moreDestinations > 0) {
            details.push(
              <tr key={`${key}|more`} className="table__member">
                <td colSpan={5} className="muted">
                  +{row.moreDestinations} more {row.moreDestinations === 1 ? 'destination' : 'destinations'}
                </td>
              </tr>,
            );
          }
          return [main, ...details];
        })}
      </tbody>
    </table>
  );
}

/** The destinations of the range grouped by the network that owns them: who the traffic went to, not what name was asked for. */
export function TopOwners({
  rangeSecs,
  allAddresses,
  refreshKey,
  onSelect,
}: {
  rangeSecs: number;
  allAddresses: boolean;
  /** Changes when the screen should read again (for example after owner data was downloaded). */
  refreshKey: number;
  onSelect: (addr: string) => void;
}) {
  const api = useApi();
  const sort = useSort();
  const result = usePolled(
    () => api.topOwners(rangeSecs, undefined, allAddresses),
    REFRESH_MS,
    `owners:${rangeSecs}:${allAddresses}:${refreshKey}`,
  );
  return (
    <section className="card card--wide">
      <h2>Top owners</h2>
      <p className="muted">
        Destinations grouped by the organisation that runs their network range. Open an owner to see its biggest
        destinations.
      </p>
      <DataGate result={result}>
        {(data) => (
          <>
            {data.historyStartsMs !== null && data.historyStartsMs > data.fromMs && (
              <p className="muted">History begins {formatDay(data.historyStartsMs)}. Earlier time was not collected.</p>
            )}
            {data.ownerData === null && (
              <p className="muted">
                There is no owner data on this machine yet, so every destination is in the group with no owner found.
                Download owner data above to see the owners.
              </p>
            )}
            {data.owners.length === 0 ? (
              <p className="muted">No destinations to group for this period.</p>
            ) : (
              <OwnersTable view={data} sort={sort} onSelect={onSelect} />
            )}
          </>
        )}
      </DataGate>
    </section>
  );
}
