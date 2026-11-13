import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ProgramMetric, ProgramRowView, ProgramsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { DEFAULT_HISTORY_RANGE_SECS, HISTORY_RANGES } from '../components/historyRanges';
import { RangePicker } from '../components/RangePicker';
import { formatBytes, formatDateTime, formatDay, formatPercent } from '../format';
import { usePolled } from '../hooks/usePolled';
import { ProgramDetail } from './ProgramDetail';

const REFRESH_MS = 60_000;

const METRICS: Array<{ id: ProgramMetric; label: string }> = [
  { id: 'cpu', label: 'CPU' },
  { id: 'memory', label: 'Memory' },
  { id: 'starts', label: 'Starts' },
  { id: 'network', label: 'Network' },
];

function ProgramName({ program, onSelect }: { program: ProgramRowView; onSelect: (program: ProgramRowView) => void }) {
  if (program.path === null) {
    return <>{program.name}</>;
  }
  return (
    <button type="button" className="linklike" onClick={() => onSelect(program)}>
      {program.name}
    </button>
  );
}

function ProgramsTable({ view, onSelect }: { view: ProgramsView; onSelect: (program: ProgramRowView) => void }) {
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Program</th>
          <th className="num">CPU average</th>
          <th className="num">CPU peak</th>
          <th className="num">Memory peak</th>
          <th className="num">Starts</th>
          <th className="num">Received</th>
          <th className="num">Sent</th>
          <th className="num">Recorded in</th>
          <th>Last seen</th>
        </tr>
      </thead>
      <tbody>
        {view.programs.map((program) => (
          <tr key={program.path ?? 'unknown'}>
            <td>
              <ProgramName program={program} onSelect={onSelect} />
            </td>
            <td className="num">{formatPercent(program.cpuAvgPercent)}</td>
            <td className="num">{formatPercent(program.cpuPeakPercent)}</td>
            <td className="num">{formatBytes(program.memoryPeakBytes)}</td>
            <td className="num">{program.starts}</td>
            <td className="num">{formatBytes(program.bytesReceived)}</td>
            <td className="num">{formatBytes(program.bytesSent)}</td>
            <td className="num">
              {program.hoursPresent} of {view.hoursInRange} h
            </td>
            <td>{program.lastSeenMs === null ? '—' : formatDateTime(program.lastSeenMs)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Programs ranked over days and months by CPU, memory, starts or network traffic, from the hourly summaries. */
export function ProgramsHistory() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_HISTORY_RANGE_SECS);
  const [metric, setMetric] = useState<ProgramMetric>('cpu');
  const [selected, setSelected] = useState<ProgramRowView | null>(null);
  const result = usePolled(() => api.topPrograms(rangeSecs, metric), REFRESH_MS, `programs:${rangeSecs}:${metric}`);

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} options={HISTORY_RANGES} />
        <div className="range" role="group" aria-label="Rank by">
          {METRICS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              className="range__button"
              aria-pressed={metric === entry.id}
              onClick={() => setMetric(entry.id)}
            >
              {entry.label}
            </button>
          ))}
        </div>
      </div>
      <DataGate result={result}>
        {(data) => (
          <>
            {data.historyStartsMs !== null && data.historyStartsMs > data.fromMs && (
              <p className="muted">History begins {formatDay(data.historyStartsMs)}. Earlier time was not collected.</p>
            )}
            {data.programs.length === 0 ? (
              <p className="muted">No programs to rank for this metric in this period.</p>
            ) : (
              <section className="card card--wide">
                <ProgramsTable view={data} onSelect={setSelected} />
              </section>
            )}
          </>
        )}
      </DataGate>
      {selected && selected.path !== null && (
        <ProgramDetail
          name={selected.name}
          path={selected.path}
          rangeSecs={rangeSecs}
          onClose={() => setSelected(null)}
        />
      )}
      <p className="muted">
        CPU and memory are recorded only while a program is among the 25 busiest processes, so small programs can look
        absent. The CPU average counts those times as zero. Network excludes traffic to this machine itself, and the
        unknown-program row holds processes whose program could not be identified.
      </p>
    </>
  );
}
