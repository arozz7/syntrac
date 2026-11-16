import { useMemo } from 'react';
import { useApi } from '../api/ApiContext';
import type { DestinationHistoryView } from '../api/types';
import { granularityFor } from '../charts/bucketing';
import { bucketDestinationHours, type DestinationBucket } from '../charts/destinationBuckets';
import { buildBytesChartOption } from '../charts/historyChart';
import { readPalette } from '../charts/readPalette';
import { Chart } from '../components/Chart';
import { DataGate } from '../components/DataGate';
import { SortHeader } from '../components/SortHeader';
import { formatBytes, formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { useSort } from '../hooks/useSort';
import type { Accessors } from '../sorting';
import { classLabel, ownerLine } from '../networkLabels';
import { ProgramLink } from './ProgramLink';

const REFRESH_MS = 60_000;

const SORTABLE: Accessors<DestinationHistoryView['programs'][number]> = {
  program: (program) => program.name,
  received: (program) => program.bytesReceived,
  sent: (program) => program.bytesSent,
  flows: (program) => program.flows,
  lastSeen: (program) => program.lastSeenMs,
};

function DetailBody({
  data,
  onOpenProgram,
}: {
  data: DestinationHistoryView;
  onOpenProgram: (program: { name: string; path: string }) => void;
}) {
  const sort = useSort();
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
        {data.owner !== null && (
          <div>
            <dt>Network owner</dt>
            <dd>{ownerLine(data.owner)}</dd>
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
              <SortHeader sort={sort} column="program" label="Program" />
              <SortHeader sort={sort} column="received" label="Received" firstDirection="desc" className="num" />
              <SortHeader sort={sort} column="sent" label="Sent" firstDirection="desc" className="num" />
              <SortHeader sort={sort} column="flows" label="Flows" firstDirection="desc" className="num" />
              <SortHeader sort={sort} column="lastSeen" label="Last seen" firstDirection="desc" />
            </tr>
          </thead>
          <tbody>
            {sort.apply(data.programs, SORTABLE).map((program) => (
              <tr key={program.path ?? program.name}>
                <td>
                  {program.path === null ? (
                    program.name
                  ) : (
                    <ProgramLink name={program.name} path={program.path} onOpen={onOpenProgram} />
                  )}
                </td>
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
  onOpenProgram,
}: {
  addr: string;
  rangeSecs: number;
  onOpenProgram: (program: { name: string; path: string }) => void;
}) {
  const api = useApi();
  const result = usePolled(
    () => api.destinationHistory(rangeSecs, addr),
    REFRESH_MS,
    `destination:${addr}:${rangeSecs}`,
  );
  return <DataGate result={result}>{(data) => <DetailBody data={data} onOpenProgram={onOpenProgram} />}</DataGate>;
}
