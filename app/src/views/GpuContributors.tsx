import type { GpuView } from '../api/types';
import { formatBytes, formatPercent } from '../format';

/**
 * Who used the GPU in the period. Every figure is a share of ONE GPU engine (the program's busiest, what Task Manager shows), not of
 * the whole machine, so the rows do not add up to the system figure and there is no "unaccounted" row. A period in which the GPU
 * counters were not available says so: it is never shown as an idle GPU.
 */
export function GpuContributors({ gpu }: { gpu: GpuView }) {
  if (gpu.readingSamples === 0) {
    return (
      <p className="muted">
        GPU use is not recorded for this period: the counters were not available, or the service that records them has
        not been running with GPU recording yet.
      </p>
    );
  }
  const widest = Math.max(...gpu.processes.map((p) => p.avgPercent), 0.1);
  return (
    <>
      <p className="muted">
        System average {formatPercent(gpu.systemAvgPercent)} of the busiest GPU engine, peak{' '}
        {formatPercent(gpu.systemPeakPercent)}, over {gpu.readingSamples} samples. Each program&apos;s figure is a share
        of its own busiest engine, not of the machine, so the rows do not add up to it.
      </p>
      {gpu.processes.length === 0 ? (
        <p className="muted">No program used the GPU in this period.</p>
      ) : (
        <ul className="bars">
          {gpu.processes.map((program) => (
            <li key={`${program.instance ?? 'pid'}-${program.pid}`} className="bars__row bars__row--process">
              <span className="bars__name">
                {program.name}{' '}
                {!program.name.startsWith('pid ') && <span className="muted">{`pid ${program.pid}`}</span>}
              </span>
              <span className="bars__track" aria-hidden="true">
                <span
                  className="bars__fill"
                  style={{ width: `${Math.min(100, (program.avgPercent / widest) * 100)}%` }}
                />
              </span>
              <span className="bars__value">{formatPercent(program.avgPercent)}</span>
              <span className="bars__peak muted">peak {formatPercent(program.peakPercent)}</span>
              {program.memoryPeakBytes > 0 && (
                <span className="bars__note muted">{formatBytes(program.memoryPeakBytes)} GPU memory (peak)</span>
              )}
            </li>
          ))}
        </ul>
      )}
      {gpu.othersPeakPercent > 0 && (
        <p className="muted">Programs not listed used at most {formatPercent(gpu.othersPeakPercent)} each.</p>
      )}
    </>
  );
}
