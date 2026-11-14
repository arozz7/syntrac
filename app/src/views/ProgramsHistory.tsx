import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ProgramMetric, ProgramRowView, ProgramsView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { IdentityCell } from '../components/IdentityBadge';
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

/** A program whose history can be opened. */
interface Choice {
  name: string;
  path: string;
}

function ProgramName({
  program,
  expanded,
  onSelect,
  onToggle,
}: {
  program: ProgramRowView;
  expanded: boolean;
  onSelect: (choice: Choice) => void;
  onToggle: () => void;
}) {
  if (program.members.length > 0) {
    return (
      <>
        <button type="button" className="linklike" aria-expanded={expanded} onClick={onToggle}>
          {program.name}
        </button>{' '}
        <span className="muted">{program.members.length} programs</span>
      </>
    );
  }
  if (program.path === null) {
    return <>{program.name}</>;
  }
  const path = program.path;
  return (
    <button type="button" className="linklike" onClick={() => onSelect({ name: program.name, path })}>
      {program.name}
    </button>
  );
}

function ProgramsTable({ view, onSelect }: { view: ProgramsView; onSelect: (choice: Choice) => void }) {
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const toggle = (name: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (!next.delete(name)) {
        next.add(name);
      }
      return next;
    });

  return (
    <table className="table">
      <thead>
        <tr>
          <th>Program</th>
          <th>Identity</th>
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
        {view.programs.flatMap((program) => {
          const key = program.path ?? program.name;
          const expanded = open.has(key);
          const row = (
            <tr key={key}>
              <td>
                <ProgramName program={program} expanded={expanded} onSelect={onSelect} onToggle={() => toggle(key)} />
              </td>
              <td>
                <IdentityCell identity={program.identity} standing={program.standing} />
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
          );
          const members = expanded
            ? program.members.map((member) => (
                <tr key={`${key}|${member.path}`} className="table__member">
                  <td>
                    <button
                      type="button"
                      className="linklike"
                      onClick={() => onSelect({ name: member.name, path: member.path })}
                    >
                      {member.name}
                    </button>
                  </td>
                  <td>
                    <IdentityCell identity={member.identity} standing={member.standing} />
                  </td>
                  <td colSpan={8} />
                </tr>
              ))
            : [];
          return [row, ...members];
        })}
      </tbody>
    </table>
  );
}

/** Programs ranked over days and months by CPU, memory, starts or network traffic, from the hourly summaries. */
export function ProgramsHistory() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_HISTORY_RANGE_SECS);
  const [metric, setMetric] = useState<ProgramMetric>('cpu');
  const [grouped, setGrouped] = useState(false);
  const [selected, setSelected] = useState<Choice | null>(null);
  // Bumped when the user changes the list of known programs, so the ranking and the open detail are read again at once.
  const [version, setVersion] = useState(0);
  const result = usePolled(
    () => api.topPrograms(rangeSecs, metric, undefined, grouped),
    REFRESH_MS,
    `programs:${rangeSecs}:${metric}:${grouped}:${version}`,
  );

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
        <div className="range" role="group" aria-label="Grouping">
          <button
            type="button"
            className="range__button"
            aria-pressed={grouped}
            onClick={() => setGrouped((current) => !current)}
          >
            Group by product
          </button>
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
      {selected && (
        <ProgramDetail
          name={selected.name}
          path={selected.path}
          rangeSecs={rangeSecs}
          onClose={() => setSelected(null)}
          onChanged={() => setVersion((current) => current + 1)}
          version={version}
        />
      )}
      <p className="muted">
        CPU and memory are recorded only while a program is among the 25 busiest processes, so small programs can look
        absent. The CPU average counts those times as zero. Network excludes traffic to this machine itself, and the
        unknown-program row holds processes whose program could not be identified. The identity column is evidence read
        from each program&apos;s file, not a verdict: a signature shows who signed a file, not that it is safe. When
        grouped, CPU averages, starts and traffic of a product are added up; its peaks are the highest of any one
        program.
      </p>
    </>
  );
}
