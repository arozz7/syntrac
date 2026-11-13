import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { DestinationView, OverviewView } from '../api/types';
import { RangePicker } from '../components/RangePicker';
import { DataGate } from '../components/DataGate';
import { formatBytes, formatPercent } from '../format';
import { usePolled } from '../hooks/usePolled';
import { CpuContributors } from './CpuContributors';
import { SystemChart } from './SystemChart';

const REFRESH_MS = 5000;
const DEFAULT_RANGE_SECS = 900;

function endpoint(destination: DestinationView): string {
  return `${destination.remoteAddr.includes(':') ? `[${destination.remoteAddr}]` : destination.remoteAddr}:${destination.remotePort}`;
}

function sum(values: number[]): number {
  return values.reduce((total, value) => total + value, 0);
}

function Destinations({ destinations }: { destinations: DestinationView[] }) {
  if (destinations.length === 0) {
    return <p className="muted">No network activity in this period.</p>;
  }
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Destination</th>
          <th className="num">Received</th>
          <th className="num">Sent</th>
          <th className="num">Flows</th>
        </tr>
      </thead>
      <tbody>
        {destinations.map((d) => (
          <tr key={endpoint(d)}>
            <td>{endpoint(d)}</td>
            <td className="num">{formatBytes(d.bytesReceived)}</td>
            <td className="num">{formatBytes(d.bytesSent)}</td>
            <td className="num">{d.flowCount}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function OverviewBody({ data }: { data: OverviewView }) {
  const latest = data.system.at(-1);
  return (
    <div className="grid">
      <section className="card card--wide">
        <h2>System</h2>
        {data.system.length === 0 ? (
          <p className="muted">No samples in this period yet.</p>
        ) : (
          <>
            <SystemChart points={data.system} />
            <dl className="facts">
              <div>
                <dt>CPU now</dt>
                <dd>{latest ? formatPercent(latest.cpuPercent) : '—'}</dd>
              </div>
              <div>
                <dt>Memory now</dt>
                <dd>{latest ? formatPercent(latest.memoryPercent) : '—'}</dd>
              </div>
              <div>
                <dt>Network in</dt>
                <dd>{formatBytes(sum(data.system.map((p) => p.netReceivedBytes)))}</dd>
              </div>
              <div>
                <dt>Network out</dt>
                <dd>{formatBytes(sum(data.system.map((p) => p.netSentBytes)))}</dd>
              </div>
            </dl>
          </>
        )}
      </section>

      <section className="card">
        <h2>Who is using the CPU</h2>
        <CpuContributors cpu={data.cpu} />
      </section>

      <section className="card">
        <h2>Network destinations</h2>
        <p className="muted">Excludes traffic to this machine itself.</p>
        <Destinations destinations={data.destinations} />
      </section>
    </div>
  );
}

export function Overview() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_RANGE_SECS);
  const result = usePolled(() => api.overview(rangeSecs), REFRESH_MS, `overview:${rangeSecs}`);

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} />
      </div>
      <DataGate result={result}>{(data) => <OverviewBody data={data} />}</DataGate>
    </>
  );
}
