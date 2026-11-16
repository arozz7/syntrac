import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ConnectionRowView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { RangePicker } from '../components/RangePicker';
import { SortHeader } from '../components/SortHeader';
import { formatBytes, formatClock } from '../format';
import { usePolled } from '../hooks/usePolled';
import { useSort, type SortControl } from '../hooks/useSort';
import type { Accessors } from '../sorting';

const REFRESH_MS = 5000;
const DEFAULT_RANGE_SECS = 900;
const ROW_LIMIT = 200;

function remote(row: ConnectionRowView): string {
  return `${row.remoteAddr.includes(':') ? `[${row.remoteAddr}]` : row.remoteAddr}:${row.remotePort}`;
}

const SORTABLE: Accessors<ConnectionRowView> = {
  time: (row) => row.startMs,
  process: (row) => row.name,
  protocol: (row) => row.protocol,
  remote: (row) => remote(row),
  sent: (row) => row.bytesSent,
  received: (row) => row.bytesReceived,
  opened: (row) => row.opened,
  failed: (row) => row.failed,
};

function ConnectionTable({ rows, sort }: { rows: ConnectionRowView[]; sort: SortControl }) {
  if (rows.length === 0) {
    return <p className="muted">No connections in this period.</p>;
  }
  return (
    <table className="table">
      <thead>
        <tr>
          <SortHeader sort={sort} column="time" label="Time" firstDirection="desc" />
          <SortHeader sort={sort} column="process" label="Process" />
          <SortHeader sort={sort} column="protocol" label="Protocol" />
          <SortHeader sort={sort} column="remote" label="Remote" />
          <SortHeader sort={sort} column="sent" label="Sent" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="received" label="Received" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="opened" label="Opened" firstDirection="desc" className="num" />
          <SortHeader sort={sort} column="failed" label="Failed" firstDirection="desc" className="num" />
        </tr>
      </thead>
      <tbody>
        {sort.apply(rows, SORTABLE).map((row, index) => (
          <tr key={`${row.startMs}-${row.pid}-${remote(row)}-${index}`}>
            <td>{formatClock(row.startMs)}</td>
            <td>{row.name}</td>
            <td>{row.protocol.toUpperCase()}</td>
            <td>{remote(row)}</td>
            <td className="num">{formatBytes(row.bytesSent)}</td>
            <td className="num">{formatBytes(row.bytesReceived)}</td>
            <td className="num">{row.opened}</td>
            <td className="num">{row.failed}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function Connections() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_RANGE_SECS);
  const sort = useSort();
  const result = usePolled(() => api.connections(rangeSecs, ROW_LIMIT), REFRESH_MS, `connections:${rangeSecs}`);

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} />
      </div>
      <DataGate result={result}>{(rows) => <ConnectionTable rows={rows} sort={sort} />}</DataGate>
    </>
  );
}
