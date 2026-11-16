import { useMemo } from 'react';
import { useApi } from '../api/ApiContext';
import type { ProgramHistoryView } from '../api/types';
import { granularityFor } from '../charts/bucketing';
import { buildBytesChartOption } from '../charts/historyChart';
import { bucketProgramHours, type ProgramBucket } from '../charts/programBuckets';
import { buildProgramCpuChartOption } from '../charts/programChart';
import { readPalette } from '../charts/readPalette';
import { Chart } from '../components/Chart';
import { DataGate } from '../components/DataGate';
import { IdentityPanel } from '../components/IdentityPanel';
import { formatBytes, formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { ProgramDestinations } from './ProgramDestinations';

const REFRESH_MS = 60_000;

function DetailBody({
  data,
  name,
  rangeSecs,
  onChanged,
  onOpenDestination,
}: {
  data: ProgramHistoryView;
  name: string;
  rangeSecs: number;
  onChanged: () => void;
  onOpenDestination: ((addr: string) => void) | undefined;
}) {
  const granularity = granularityFor((data.toMs - data.fromMs) / 1000);
  const buckets = useMemo(() => bucketProgramHours(data.hours, granularity), [data.hours, granularity]);
  const palette = useMemo(() => readPalette(), []);
  const cpu = useMemo(() => buildProgramCpuChartOption(buckets, granularity, palette), [buckets, granularity, palette]);
  const memory = useMemo(
    () =>
      buildBytesChartOption<ProgramBucket>(
        buckets,
        granularity,
        [{ name: 'Memory peak', pick: (b) => b.memoryPeak }],
        palette,
      ),
    [buckets, granularity, palette],
  );
  const starts = data.hours.reduce((total, hour) => total + hour.starts, 0);
  const present = data.hours.filter((hour) => hour.present).length;
  const peakMemory = data.hours.reduce((highest, hour) => Math.max(highest, hour.memoryPeakBytes), 0);

  return (
    <>
      <dl className="facts">
        <div>
          <dt>First seen</dt>
          <dd>{data.firstSeenMs === null ? '—' : formatDateTime(data.firstSeenMs)}</dd>
        </div>
        <div>
          <dt>Last seen</dt>
          <dd>{data.lastSeenMs === null ? '—' : formatDateTime(data.lastSeenMs)}</dd>
        </div>
        <div>
          <dt>Starts in this range</dt>
          <dd>{starts}</dd>
        </div>
        <div>
          <dt>Recorded in</dt>
          <dd>
            {present} of {data.hours.length} h
          </dd>
        </div>
        <div>
          <dt>Memory peak</dt>
          <dd>{formatBytes(peakMemory)}</dd>
        </div>
      </dl>
      <Chart option={cpu} label="Program CPU over time" />
      <Chart option={memory} label="Program memory over time" />
      <ProgramDestinations path={data.path} rangeSecs={rangeSecs} onOpenDestination={onOpenDestination} />
      <IdentityPanel
        identity={data.identity}
        standing={data.standing}
        name={name}
        path={data.path}
        onChanged={onChanged}
      />
    </>
  );
}

/** One program, hour by hour (or day by day for long ranges): the path and everything below it, without a frame around it. */
export function ProgramDetailBody({
  name,
  path,
  rangeSecs,
  onChanged,
  onOpenDestination,
  version = 0,
}: {
  name: string;
  path: string;
  rangeSecs: number;
  /** Called after the user changed something about this program (for example marked it as known). */
  onChanged: () => void;
  /** Makes the addresses in "Where it connected" links that open them. Left out where the page has nowhere to open one. */
  onOpenDestination?: (addr: string) => void;
  /** Changes whenever the list of known programs changed, so the screen is read again. */
  version?: number;
}) {
  const api = useApi();
  const result = usePolled(
    () => api.programHistory(rangeSecs, path),
    REFRESH_MS,
    `program:${path}:${rangeSecs}:${version}`,
  );
  return (
    <>
      <p className="muted">{path}</p>
      <DataGate result={result}>
        {(data) => (
          <DetailBody
            data={data}
            name={name}
            rangeSecs={rangeSecs}
            onChanged={onChanged}
            onOpenDestination={onOpenDestination}
          />
        )}
      </DataGate>
    </>
  );
}

/** One program below the ranking, in a card of its own with a Close button. */
export function ProgramDetail({
  name,
  path,
  rangeSecs,
  onClose,
  onChanged,
  version = 0,
}: {
  name: string;
  path: string;
  rangeSecs: number;
  onClose: () => void;
  onChanged: () => void;
  version?: number;
}) {
  return (
    <section className="card card--wide">
      <div className="card__head">
        <h2>{name}</h2>
        <button type="button" className="range__button" onClick={onClose}>
          Close
        </button>
      </div>
      <ProgramDetailBody name={name} path={path} rangeSecs={rangeSecs} onChanged={onChanged} version={version} />
    </section>
  );
}
