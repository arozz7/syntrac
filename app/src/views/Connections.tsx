import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ConnectionRowView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { RangePicker } from '../components/RangePicker';
import { formatBytes, formatClock } from '../format';
import { usePolled } from '../hooks/usePolled';

const REFRESH_MS = 5000;
const DEFAULT_RANGE_SECS = 900;
const ROW_LIMIT = 200;

function remote(row: ConnectionRowView): string {
  return `${row.remoteAddr.includes(':') ? `[${row.remoteAddr}]` : row.remoteAddr}:${row.remotePort}`;
}

function ConnectionTable({ rows }: { rows: ConnectionRowView[] }) {
  if (rows.length === 0) {
    return <p className="muted">No connections in this period.</p>;
  }
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Time</th>
          <th>Process</th>
          <th>Protocol</th>
          <th>Remote</th>
          <th className="num">Sent</th>
          <th className="num">Received</th>
          <th className="num">Opened</th>
          <th className="num">Failed</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => (
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
  const result = usePolled(() => api.connections(rangeSecs, ROW_LIMIT), REFRESH_MS, `connections:${rangeSecs}`);

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} />
      </div>
      <DataGate result={result}>{(rows) => <ConnectionTable rows={rows} />}</DataGate>
    </>
  );
}
