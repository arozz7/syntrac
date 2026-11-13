import { useMemo, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { SystemHistoryView } from '../api/types';
import { bucketHours, granularityFor, summarise } from '../charts/historyBuckets';
import { buildBytesChartOption, buildLoadChartOption } from '../charts/historyChart';
import { readPalette } from '../charts/readPalette';
import { Chart } from '../components/Chart';
import { DataGate } from '../components/DataGate';
import { RangePicker, type RangeOption } from '../components/RangePicker';
import { formatBytes, formatDay, formatPercent } from '../format';
import { usePolled } from '../hooks/usePolled';

const REFRESH_MS = 60_000;
const DAY_SECS = 86_400;
const DEFAULT_RANGE_SECS = 7 * DAY_SECS;

/** "All" is simply longer than any history (ten years, the most the backend will look back). */
export const HISTORY_RANGES: RangeOption[] = [
  { label: '24 hours', secs: DAY_SECS },
  { label: '7 days', secs: 7 * DAY_SECS },
  { label: '30 days', secs: 30 * DAY_SECS },
  { label: '90 days', secs: 90 * DAY_SECS },
  { label: '1 year', secs: 365 * DAY_SECS },
  { label: 'All', secs: 3650 * DAY_SECS },
];

function HistoryBody({ data }: { data: SystemHistoryView }) {
  const granularity = granularityFor((data.toMs - data.fromMs) / 1000);
  const buckets = useMemo(() => bucketHours(data.hours, granularity), [data.hours, granularity]);
  const summary = useMemo(() => summarise(buckets), [buckets]);
  const palette = useMemo(() => readPalette(), []);
  const load = useMemo(() => buildLoadChartOption(buckets, granularity, palette), [buckets, granularity, palette]);
  const network = useMemo(
    () =>
      buildBytesChartOption(
        buckets,
        granularity,
        [
          { name: 'Received', pick: (b) => b.netReceived },
          { name: 'Sent', pick: (b) => b.netSent },
        ],
        palette,
      ),
    [buckets, granularity, palette],
  );
  const disk = useMemo(
    () =>
      buildBytesChartOption(
        buckets,
        granularity,
        [
          { name: 'Read', pick: (b) => b.diskRead },
          { name: 'Written', pick: (b) => b.diskWrite },
        ],
        palette,
      ),
    [buckets, granularity, palette],
  );

  if (!summary) {
    return (
      <p className="muted">
        No hourly summaries yet. The collector writes one about 5 minutes after each hour ends, so the first appears
        within the hour.
      </p>
    );
  }
  const startsLate = data.historyStartsMs !== null && data.historyStartsMs > data.fromMs;
  return (
    <>
      {startsLate && data.historyStartsMs !== null && (
        <p className="muted">History begins {formatDay(data.historyStartsMs)}. Earlier time was not collected.</p>
      )}
      <div className="grid">
        <section className="card card--wide">
          <h2>CPU and memory</h2>
          <Chart option={load} label="CPU and memory over time" />
          <dl className="facts">
            <div>
              <dt>CPU average</dt>
              <dd>{formatPercent(summary.cpuAvg)}</dd>
            </div>
            <div>
              <dt>CPU peak</dt>
              <dd>{formatPercent(summary.cpuMax)}</dd>
            </div>
            <div>
              <dt>Memory average</dt>
              <dd>{formatPercent(summary.memoryAvg)}</dd>
            </div>
            <div>
              <dt>Memory peak</dt>
              <dd>{formatPercent(summary.memoryMax)}</dd>
            </div>
            <div>
              <dt>Hours of data</dt>
              <dd>{summary.hours}</dd>
            </div>
          </dl>
        </section>

        <section className="card">
          <h2>Network</h2>
          <Chart option={network} label="Network traffic over time" />
          <dl className="facts">
            <div>
              <dt>Received</dt>
              <dd>{formatBytes(summary.netReceived)}</dd>
            </div>
            <div>
              <dt>Sent</dt>
              <dd>{formatBytes(summary.netSent)}</dd>
            </div>
          </dl>
        </section>

        <section className="card">
          <h2>Disk</h2>
          <Chart option={disk} label="Disk traffic over time" />
          <dl className="facts">
            <div>
              <dt>Read</dt>
              <dd>{formatBytes(summary.diskRead)}</dd>
            </div>
            <div>
              <dt>Written</dt>
              <dd>{formatBytes(summary.diskWrite)}</dd>
            </div>
          </dl>
        </section>
      </div>
      <p className="muted">
        Only completed hours are shown; the latest hour appears about 5 minutes after it ends. A break in a line is time
        when nothing was collected, not zero.
      </p>
    </>
  );
}

/** The machine over days and months, from the hourly summaries that outlive the detailed data. */
export function SystemHistory() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_RANGE_SECS);
  const result = usePolled(() => api.systemHistory(rangeSecs), REFRESH_MS, `system-history:${rangeSecs}`);

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} options={HISTORY_RANGES} />
      </div>
      <DataGate result={result}>{(data) => <HistoryBody data={data} />}</DataGate>
    </>
  );
}
