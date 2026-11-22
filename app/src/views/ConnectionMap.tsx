import { useMemo, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ConnectionMapView, MapNetworkView } from '../api/types';
import { buildConnectionMapOption, mapChartHeight, type MapNodeRef } from '../charts/mapChart';
import { readPalette } from '../charts/readPalette';
import { Chart } from '../components/Chart';
import { DataGate } from '../components/DataGate';
import { RangePicker } from '../components/RangePicker';
import { DEFAULT_HISTORY_RANGE_SECS, HISTORY_RANGES } from '../components/historyRanges';
import { formatBytes, formatDay } from '../format';
import { usePolled } from '../hooks/usePolled';
import { ownerLine } from '../networkLabels';
import { NetworkDialog, type NetworkFocus } from './NetworkDialog';

const REFRESH_MS = 60_000;

type OpenProgram = (program: { name: string; path: string }) => void;

function isNodeRef(data: unknown): data is MapNodeRef {
  if (typeof data !== 'object' || data === null) {
    return false;
  }
  const { side, index } = data as { side?: unknown; index?: unknown };
  return (side === 'program' || side === 'network') && typeof index === 'number';
}

/** How much of the traffic the chart shows, in words, when the busiest were picked out of more. */
function cutNote(view: ConnectionMapView): string | null {
  const programs = view.programs.filter((node) => node.kind !== 'others').length;
  const networks = view.networks.filter((node) => node.kind !== 'others').length;
  const parts: string[] = [];
  if (view.totalPrograms > programs) {
    parts.push(`the ${programs} busiest of ${view.totalPrograms} programs`);
  }
  if (view.totalNetworks > networks) {
    parts.push(`the ${networks} busiest of ${view.totalNetworks} networks`);
  }
  return parts.length === 0
    ? null
    : `Showing ${parts.join(' and ')}; the rest are added together in one node on each side.`;
}

function NetworkPanel({ network, onOpenAddress }: { network: MapNetworkView; onOpenAddress: (addr: string) => void }) {
  return (
    <section className="card card--wide" aria-label={network.label}>
      <h3>{network.label}</h3>
      <p className="muted">
        {network.owner === null ? 'No owner was found for these addresses.' : ownerLine(network.owner)}. Sent{' '}
        {formatBytes(network.bytesSent)}, received {formatBytes(network.bytesReceived)}, {network.addresses}{' '}
        {network.addresses === 1 ? 'address' : 'addresses'}.
      </p>
      {network.top.length > 0 && (
        <ul className="plain-list">
          {network.top.map((address) => (
            <li key={address.addr}>
              <button type="button" className="linklike" onClick={() => onOpenAddress(address.addr)}>
                {address.addr}
              </button>
              {address.name !== null && <span className="muted"> {address.name}</span>}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function LinkTable({
  view,
  onOpenProgram,
  onSelectNetwork,
}: {
  view: ConnectionMapView;
  onOpenProgram: OpenProgram;
  onSelectNetwork: (index: number) => void;
}) {
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Program</th>
          <th>Network</th>
          <th className="num">Sent</th>
          <th className="num">Received</th>
        </tr>
      </thead>
      <tbody>
        {view.links.map((link) => {
          const program = view.programs[link.program];
          const network = view.networks[link.network];
          if (program === undefined || network === undefined) {
            return null;
          }
          return (
            <tr key={`${link.program}:${link.network}`}>
              <td>
                {program.path === null ? (
                  program.name
                ) : (
                  <button
                    type="button"
                    className="linklike"
                    title={program.path}
                    onClick={() => onOpenProgram({ name: program.name, path: program.path ?? '' })}
                  >
                    {program.name}
                  </button>
                )}
              </td>
              <td>
                {network.kind === 'others' ? (
                  network.label
                ) : (
                  <button type="button" className="linklike" onClick={() => onSelectNetwork(link.network)}>
                    {network.label}
                  </button>
                )}
              </td>
              <td className="num">{formatBytes(link.bytesSent)}</td>
              <td className="num">{formatBytes(link.bytesReceived)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function MapBody({
  view,
  onOpenProgram,
  onOpenAddress,
}: {
  view: ConnectionMapView;
  onOpenProgram: OpenProgram;
  onOpenAddress: (addr: string) => void;
}) {
  const palette = useMemo(() => readPalette(), []);
  const option = useMemo(() => buildConnectionMapOption(view, palette), [view, palette]);
  const [network, setNetwork] = useState<number | null>(null);
  const note = cutNote(view);
  const chosen = network === null ? undefined : view.networks[network];

  const select = (data: unknown) => {
    if (!isNodeRef(data)) {
      return;
    }
    if (data.side === 'program') {
      const program = view.programs[data.index];
      if (program?.path) {
        onOpenProgram({ name: program.name, path: program.path });
      }
    } else if (view.networks[data.index]?.kind !== 'others') {
      setNetwork(data.index);
    }
  };

  if (view.links.length === 0) {
    return <p className="muted">No traffic to show for this period.</p>;
  }
  return (
    <>
      {view.historyStartsMs !== null && view.historyStartsMs > view.fromMs && (
        <p className="muted">History begins {formatDay(view.historyStartsMs)}. Earlier time was not collected.</p>
      )}
      {note !== null && <p className="muted">{note}</p>}
      <section className="card card--wide">
        <Chart
          option={option}
          label="Which programs talked to which networks, as bands as wide as the traffic. The same information is in the table below."
          onSelect={select}
          height={mapChartHeight(view)}
        />
      </section>
      {chosen !== undefined && <NetworkPanel network={chosen} onOpenAddress={onOpenAddress} />}
      <section className="card card--wide">
        <h3>As a table</h3>
        <LinkTable view={view} onOpenProgram={onOpenProgram} onSelectNetwork={setNetwork} />
      </section>
    </>
  );
}

/** Which programs talked to which networks over a range: the busiest of each, bands as wide as the traffic. */
export function ConnectionMap() {
  const api = useApi();
  const [rangeSecs, setRangeSecs] = useState(DEFAULT_HISTORY_RANGE_SECS);
  const [allAddresses, setAllAddresses] = useState(false);
  const [focus, setFocus] = useState<NetworkFocus | null>(null);
  const result = usePolled(
    () => api.connectionMap(rangeSecs, { allAddresses }),
    REFRESH_MS,
    `map:${rangeSecs}:${allAddresses}`,
  );

  return (
    <>
      <div className="toolbar">
        <RangePicker value={rangeSecs} onChange={setRangeSecs} options={HISTORY_RANGES} />
        <div className="range" role="group" aria-label="Addresses">
          <button
            type="button"
            className="range__button"
            aria-pressed={allAddresses}
            onClick={() => setAllAddresses((current) => !current)}
          >
            Include this network and multicast
          </button>
        </div>
      </div>
      <h2>Connection map</h2>
      <DataGate result={result}>
        {(view) => (
          <MapBody
            view={view}
            onOpenProgram={(program) => setFocus({ kind: 'program', ...program, fromAddr: null })}
            onOpenAddress={(addr) => setFocus({ kind: 'destination', addr })}
          />
        )}
      </DataGate>
      {focus !== null && (
        <NetworkDialog focus={focus} rangeSecs={rangeSecs} onChange={setFocus} onClose={() => setFocus(null)} />
      )}
      <p className="muted">
        Each band is the traffic one program sent and received to the addresses of one network. A network here is the
        organisation that runs the range of addresses, which is often a cloud or hosting company: not necessarily the
        service that was reached, and not where this machine is. Addresses no range covers are shown together as no
        owner found, never guessed. Traffic to this machine itself is left out, and so are this network&apos;s own
        addresses unless you include them. Click a program for its history, or a network for its busiest addresses.
      </p>
    </>
  );
}
