import type { CpuView } from '../api/types';
import { formatPercent } from '../format';

interface Row {
  key: string;
  name: string;
  detail?: string;
  percent: number;
  peak?: number;
  kind: 'process' | 'rest' | 'unattributed';
}

/**
 * Who used the CPU, as a share of the WHOLE machine. The rows add up to the system average, with two special rows:
 * everything outside the heaviest processes, and the CPU that no process explains (interrupt/driver time and
 * processes the collector cannot read). That second row is shown rather than hidden, so a gap is never silent.
 */
export function CpuContributors({ cpu }: { cpu: CpuView }) {
  const rows: Row[] = [
    ...cpu.processes.map<Row>((p) => ({
      key: `${p.instance ?? 'pid'}-${p.pid}`,
      name: p.name,
      // A process already named by its pid ("pid 9392") would otherwise show it twice.
      detail: p.name.startsWith('pid ') ? undefined : `pid ${p.pid}`,
      percent: p.avgPercent,
      peak: p.peakPercent,
      kind: 'process',
    })),
    { key: 'rest', name: 'Everything else', percent: cpu.othersAvgPercent, kind: 'rest' },
    {
      key: 'unattributed',
      name: 'Not attributed to any process',
      percent: cpu.unaccountedAvgPercent,
      kind: 'unattributed',
    },
  ];
  const widest = Math.max(...rows.map((r) => r.percent), 0.1);

  return (
    <>
      <p className="muted">
        System average {formatPercent(cpu.systemAvgPercent)} over {cpu.windowSamples} samples. Percentages are shares of
        the whole machine.
      </p>
      <ul className="bars">
        {rows.map((row) => (
          <li key={row.key} className={`bars__row bars__row--${row.kind}`}>
            <span className="bars__name">
              {row.name} {row.detail && <span className="muted">{row.detail}</span>}
            </span>
            <span className="bars__track" aria-hidden="true">
              <span className="bars__fill" style={{ width: `${Math.min(100, (row.percent / widest) * 100)}%` }} />
            </span>
            <span className="bars__value">{formatPercent(row.percent)}</span>
            {row.peak !== undefined && <span className="bars__peak muted">peak {formatPercent(row.peak)}</span>}
            {row.kind === 'unattributed' && (
              <span className="bars__note muted">
                Interrupt and driver time, and processes the collector cannot read.
              </span>
            )}
          </li>
        ))}
      </ul>
    </>
  );
}
