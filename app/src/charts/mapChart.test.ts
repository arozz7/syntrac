import { describe, expect, it } from 'vitest';
import { connectionMapView } from '../test/fakeMap';
import type { ChartPalette } from './cpuChart';
import { buildConnectionMapOption, mapChartHeight, nodeId } from './mapChart';

const palette: ChartPalette = {
  cpu: '#111',
  memory: '#222',
  gpu: '#333',
  text: '#000',
  mutedText: '#555',
  grid: '#ccc',
  surface: '#fff',
};

interface Node {
  name: string;
  text: string;
  side: 'program' | 'network';
  index: number;
}
interface Link {
  source: string;
  target: string;
  value: number;
}
interface Series {
  type: string;
  data: Node[];
  links: Link[];
}

function series(view = connectionMapView()): Series {
  return (buildConnectionMapOption(view, palette).series as Series[])[0]!;
}

describe('buildConnectionMapOption', () => {
  it('draws a sankey with a node for every program and every network, programs first in the order given', () => {
    const drawn = series();
    expect(drawn.type).toBe('sankey');
    expect(drawn.data.map((node) => node.text)).toEqual([
      'chrome.exe',
      'code.exe',
      'EXAMPLE-NET Example Networks, Inc.',
      'No owner found',
    ]);
    expect(drawn.data.map((node) => [node.side, node.index])).toEqual([
      ['program', 0],
      ['program', 1],
      ['network', 0],
      ['network', 1],
    ]);
  });

  it('gives programs and networks ids that cannot collide, even with the same name', () => {
    expect(nodeId('program', 0)).not.toBe(nodeId('network', 0));
    const names = series().data.map((node) => node.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it('links a program to a network with the bytes sent plus received', () => {
    const drawn = series();
    expect(drawn.links).toEqual([
      { source: nodeId('program', 0), target: nodeId('network', 0), value: 110 + 4_999_000 },
      { source: nodeId('program', 1), target: nodeId('network', 0), value: 1_000_000 + 2_000 },
      { source: nodeId('program', 0), target: nodeId('network', 1), value: 1_000 },
    ]);
  });

  it('keeps the order it was given, so the busiest program is at the top', () => {
    const option = buildConnectionMapOption(connectionMapView(), palette).series as Array<{
      layoutIterations: number;
      draggable: boolean;
    }>;
    expect(option[0]!.layoutIterations).toBe(0);
    expect(option[0]!.draggable).toBe(false);
  });

  it('leaves out a link with no traffic', () => {
    const view = connectionMapView({ links: [{ program: 0, network: 0, bytesSent: 0, bytesReceived: 0 }] });
    expect(series(view).links).toEqual([]);
  });

  it('explains a link in human units when hovered', () => {
    const tooltip = (
      buildConnectionMapOption(connectionMapView(), palette).tooltip as {
        formatter: (params: unknown) => string;
      }
    ).formatter;
    const text = tooltip({
      dataType: 'edge',
      data: { source: nodeId('program', 1), target: nodeId('network', 0), value: 1_002_000 },
    });
    expect(text).toContain('code.exe');
    expect(text).toContain('EXAMPLE-NET');
    expect(text).toMatch(/MB|KB/);
  });

  it('describes a node by its totals', () => {
    const tooltip = (
      buildConnectionMapOption(connectionMapView(), palette).tooltip as {
        formatter: (params: unknown) => string;
      }
    ).formatter;
    const text = tooltip({ dataType: 'node', data: { side: 'network', index: 0 } });
    expect(text).toContain('2 addresses');
  });

  it('escapes text from outside, so a name cannot add markup to the tooltip', () => {
    const view = connectionMapView();
    view.programs[0]!.name = '<img src=x onerror=alert(1)>';
    const tooltip = (buildConnectionMapOption(view, palette).tooltip as { formatter: (params: unknown) => string })
      .formatter;
    expect(tooltip({ dataType: 'node', data: { side: 'program', index: 0 } })).not.toContain('<img');
  });
});

describe('mapChartHeight', () => {
  it('grows with the busier side and never gets too small or too large', () => {
    expect(mapChartHeight(connectionMapView())).toBeGreaterThanOrEqual(240);
    const big = connectionMapView();
    big.programs = Array.from({ length: 31 }, () => big.programs[0]!);
    expect(mapChartHeight(big)).toBeGreaterThan(mapChartHeight(connectionMapView()));
    expect(mapChartHeight(big)).toBeLessThanOrEqual(1000);
  });
});
