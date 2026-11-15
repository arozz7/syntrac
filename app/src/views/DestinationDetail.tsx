import { useMemo } from 'react';
import { useApi } from '../api/ApiContext';
import type { DestinationHistoryView } from '../api/types';
import { granularityFor } from '../charts/bucketing';
import { bucketDestinationHours, type DestinationBucket } from '../charts/destinationBuckets';
import { buildBytesChartOption } from '../charts/historyChart';
import { readPalette } from '../charts/readPalette';
import { Chart } from '../components/Chart';
import { DataGate } from '../components/DataGate';
import { formatBytes, formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { classLabel } from '../networkLabels';

const REFRESH_MS = 60_000;

function DetailBody({ data }: { data: DestinationHistoryView }) {
  const granularity = granularityFor((data.toMs - data.fromMs) / 1000);
  const buckets = useMemo(() => bucketDestinationHours(data.hours, granularity), [data.hours, granularity]);
  const palette = useMemo(() => readPalette(), []);
  const option = useMemo(
    () =>
      buildBytesChartOption<DestinationBucket>(
        buckets,
        granularity,
        [
          { name: 'Received', pick: (bucket) => bucket.bytesReceived },
          { name: 'Sent', pick: (bucket) => bucket.bytesSent },
        ],
        palette,
      ),
    [buckets, granularity, palette],
  );
  const kind = classLabel(data.class);

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
        {kind !== null && (
          <div>
            <dt>Kind</dt>
            <dd>{kind}</dd>
          </div>
        )}
      </dl>

      <h3>Names asked for</h3>
      {data.names.length === 0 ? (
        <p className="muted">
          No name was recorded for this address: a program connected to it by address, or it was reached before names
          were kept.
        </p>
      ) : (
        <ul className="names">
          {data.names.map((name) => (
            <li key={name.name}>
              <code>{name.name}</code>{' '}
              <span className="muted">
                {formatDateTime(name.firstSeenMs)} to {formatDateTime(name.lastSeenMs)}
              </span>
            </li>
          ))}
        </ul>
      )}

      <h3>Programs that talked to it</h3>
      {data.programs.length === 0 ? (
        <p className="muted">No program talked to this address in this period.</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>Program</th>
              <th className="num">Received</th>
              <th className="num">Sent</th>
              <th className="num">Flows</th>
              <th>Last seen</th>
            </tr>
          </thead>
          <tbody>
            {data.programs.map((program) => (
              <tr key={program.path ?? program.name}>
                <td>{program.name}</td>
                <td className="num">{formatBytes(program.bytesReceived)}</td>
                <td className="num">{formatBytes(program.bytesSent)}</td>
                <td className="num">{program.flows}</td>
                <td>{formatDateTime(program.lastSeenMs)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <Chart option={option} label="Traffic to this address over time" />
    </>
  );
}

/** One destination: the names asked for, who talked to it, and its traffic hour by hour (or day by day for long ranges). */
export function DestinationDetail({
  addr,
  rangeSecs,
  onClose,
}: {
  addr: string;
  rangeSecs: number;
  onClose: () => void;
}) {
  const api = useApi();
  const result = usePolled(
    () => api.destinationHistory(rangeSecs, addr),
    REFRESH_MS,
    `destination:${addr}:${rangeSecs}`,
  );
  return (
    <section className="card card--wide">
      <div className="card__head">
        <h2>{addr}</h2>
        <button type="button" className="range__button" onClick={onClose}>
          Close
        </button>
      </div>
      <DataGate result={result}>{(data) => <DetailBody data={data} />}</DataGate>
    </section>
  );
}
