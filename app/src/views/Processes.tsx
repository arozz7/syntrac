import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ProcessRowView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { formatDateTime } from '../format';
import { useDebounced } from '../hooks/useDebounced';
import { usePolled } from '../hooks/usePolled';

const REFRESH_MS = 5000;
const SEARCH_PAUSE_MS = 300;
const ROW_LIMIT = 500;

function status(row: ProcessRowView): string {
  if (row.running) {
    return 'Running';
  }
  return row.endedApproximate ? 'Exited (approx.)' : 'Exited';
}

function ProcessTable({ rows }: { rows: ProcessRowView[] }) {
  return (
    <>
      <p className="muted">
        {rows.length} {rows.length === 1 ? 'process' : 'processes'}
      </p>
      <table className="table">
        <thead>
          <tr>
            <th>Name</th>
            <th className="num">PID</th>
            <th>Status</th>
            <th>Started</th>
            <th>Ended</th>
            <th>Path</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.id}>
              <td>{row.name}</td>
              <td className="num">{row.pid}</td>
              <td>{status(row)}</td>
              <td>{formatDateTime(row.startedMs)}</td>
              <td>{row.endedMs === null ? '' : formatDateTime(row.endedMs)}</td>
              <td className="path" title={row.imagePath}>
                {row.imagePath}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}

export function Processes() {
  const api = useApi();
  const [runningOnly, setRunningOnly] = useState(false);
  const [search, setSearch] = useState('');
  const pausedSearch = useDebounced(search.trim(), SEARCH_PAUSE_MS);
  const result = usePolled(
    () => api.processes({ runningOnly, search: pausedSearch || undefined, limit: ROW_LIMIT }),
    REFRESH_MS,
    `processes:${runningOnly}:${pausedSearch}`,
  );

  return (
    <>
      <div className="toolbar">
        <input
          type="search"
          className="field"
          placeholder="Search by name or path"
          aria-label="Search processes"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <label className="check">
          <input type="checkbox" checked={runningOnly} onChange={(event) => setRunningOnly(event.target.checked)} />
          Running only
        </label>
      </div>
      <DataGate result={result}>{(rows) => <ProcessTable rows={rows} />}</DataGate>
    </>
  );
}
