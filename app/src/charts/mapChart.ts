import type { EChartsOption } from 'echarts';
import type { ConnectionMapView, MapNetworkView, MapProgramView } from '../api/types';
import { formatBytes } from '../format';
import type { ChartPalette } from './cpuChart';

// Chart options for the connection map: programs on the left, the networks that own the addresses they reached on the right, a band between
// them as wide as the traffic. Pure (colours come in as a palette), so it is unit-tested.

export type MapSide = 'program' | 'network';

/** What a chart click or hover hands back: which side, and the index into the view's `programs` or `networks`. */
export interface MapNodeRef {
  side: MapSide;
  index: number;
}

/** The id of a node: the sankey identifies nodes by name, so programs and networks get separate id spaces. */
export function nodeId(side: MapSide, index: number): string {
  return `${side === 'program' ? 'p' : 'n'}:${index}`;
}

const PX_PER_NODE = 44;
const MIN_HEIGHT = 240;
const MAX_HEIGHT = 1000;

/** Tall enough for the busier side to have a readable row per node. */
export function mapChartHeight(view: ConnectionMapView): number {
  const rows = Math.max(view.programs.length, view.networks.length);
  return Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, rows * PX_PER_NODE + 40));
}

function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function total(node: { bytesSent: number; bytesReceived: number }): number {
  return node.bytesSent + node.bytesReceived;
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

function programLine(node: MapProgramView): string {
  return `<b>${escapeHtml(node.name)}</b><br/>sent ${formatBytes(node.bytesSent)}, received ${formatBytes(node.bytesReceived)}`;
}

function networkLine(node: MapNetworkView): string {
  return (
    `<b>${escapeHtml(node.label)}</b><br/>${plural(node.addresses, 'address', 'addresses')}` +
    `<br/>sent ${formatBytes(node.bytesSent)}, received ${formatBytes(node.bytesReceived)}`
  );
}

interface TooltipParams {
  dataType?: string;
  data?: { side?: MapSide; index?: number; source?: string; target?: string; value?: number };
}

function describe(view: ConnectionMapView, params: TooltipParams): string {
  const data = params.data;
  if (data === undefined) {
    return '';
  }
  if (params.dataType === 'edge') {
    const from = view.programs[Number(data.source?.slice(2))];
    const to = view.networks[Number(data.target?.slice(2))];
    if (from === undefined || to === undefined) {
      return '';
    }
    return `${escapeHtml(from.name)} to ${escapeHtml(to.label)}<br/>${formatBytes(data.value ?? 0)} in all`;
  }
  const index = data.index ?? -1;
  if (data.side === 'program') {
    const node = view.programs[index];
    return node === undefined ? '' : programLine(node);
  }
  const node = view.networks[index];
  return node === undefined ? '' : networkLine(node);
}

/**
 * A sankey of the connection map. The order of the nodes is the order the backend ranked them in (busiest first, "others" last), so the
 * layout keeps it instead of shuffling to untangle the bands.
 */
export function buildConnectionMapOption(view: ConnectionMapView, palette: ChartPalette): EChartsOption {
  const programs = view.programs.map((node, index) => ({
    name: nodeId('program', index),
    text: node.name,
    side: 'program' as const,
    index,
    itemStyle: { color: node.kind === 'others' ? palette.mutedText : palette.cpu },
    label: { position: 'right' as const },
  }));
  const networks = view.networks.map((node, index) => ({
    name: nodeId('network', index),
    text: node.label,
    side: 'network' as const,
    index,
    itemStyle: { color: node.kind === 'others' || node.kind === 'no_owner' ? palette.mutedText : palette.memory },
    label: { position: 'left' as const },
  }));
  const links = view.links
    .filter((link) => total(link) > 0)
    .map((link) => ({
      source: nodeId('program', link.program),
      target: nodeId('network', link.network),
      value: total(link),
    }));
  return {
    animation: false,
    tooltip: {
      trigger: 'item',
      confine: true,
      formatter: (params: unknown) => describe(view, params as TooltipParams),
      backgroundColor: palette.surface,
      borderColor: palette.grid,
      textStyle: { color: palette.text },
    },
    series: [
      {
        type: 'sankey',
        left: 8,
        right: 8,
        top: 8,
        bottom: 8,
        nodeWidth: 14,
        nodeGap: 20,
        layoutIterations: 0,
        draggable: false,
        emphasis: { focus: 'adjacency' },
        lineStyle: { color: 'gradient', opacity: 0.35, curveness: 0.5 },
        label: {
          color: palette.text,
          formatter: (params: { data?: unknown }) => (params.data as { text?: string } | undefined)?.text ?? '',
        },
        data: [...programs, ...networks],
        links,
      },
    ],
  };
}
