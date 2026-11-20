import { describe, expect, it } from 'vitest';
import type { SystemPointView } from '../api/types';
import { buildCpuChartOption, type ChartPalette } from './cpuChart';

const palette: ChartPalette = {
  cpu: '#2e5d4a',
  memory: '#b07a56',
  gpu: '#4a5568',
  text: '#282b28',
  mutedText: '#4a5568',
  grid: '#cfc2a8',
  surface: '#fbf8f2',
};

const point = (tsMs: number, cpu: number, memory: number): SystemPointView => ({
  tsMs,
  cpuPercent: cpu,
  memoryPercent: memory,
  netReceivedBytes: 0,
  netSentBytes: 0,
  diskReadBytes: 0,
  diskWriteBytes: 0,
  gpuPercent: null,
});

function seriesOf(option: ReturnType<typeof buildCpuChartOption>) {
  return option.series as Array<{ name: string; data: Array<[number, number]>; lineStyle: { type?: string } }>;
}

describe('buildCpuChartOption', () => {
  it('plots CPU and memory as time/percent pairs', () => {
    const series = seriesOf(buildCpuChartOption([point(1000, 10, 40), point(2000, 20, 50)], palette));
    expect(series.map((s) => s.name)).toEqual(['CPU', 'Memory']);
    expect(series[0]?.data).toEqual([
      [1000, 10],
      [2000, 20],
    ]);
    expect(series[1]?.data).toEqual([
      [1000, 40],
      [2000, 50],
    ]);
  });

  it('pins the axis to 0-100 because both series are shares of the whole machine', () => {
    const y = buildCpuChartOption([], palette).yAxis as { min: number; max: number };
    expect([y.min, y.max]).toEqual([0, 100]);
  });

  it('distinguishes memory from CPU by line style, not only by colour', () => {
    const [cpu, memory] = seriesOf(buildCpuChartOption([point(1, 1, 1)], palette));
    expect(cpu?.lineStyle.type).toBeUndefined();
    expect(memory?.lineStyle.type).toBe('dashed');
  });

  it('gives an empty but valid option when there is no data yet', () => {
    const series = seriesOf(buildCpuChartOption([], palette));
    expect(series.every((s) => s.data.length === 0)).toBe(true);
  });

  it('takes every colour from the palette so the theme tokens stay the single source of truth', () => {
    const json = JSON.stringify(buildCpuChartOption([point(1, 1, 1)], palette));
    for (const colour of [palette.cpu, palette.memory]) {
      expect(json).toContain(colour);
    }
  });
});

describe('buildCpuChartOption: the GPU', () => {
  const withGpu = (tsMs: number, gpu: number | null): SystemPointView => ({ ...point(tsMs, 10, 40), gpuPercent: gpu });

  it('has no GPU line when no point had a reading (no counters is not an idle GPU)', () => {
    const series = seriesOf(buildCpuChartOption([point(1000, 10, 40)], palette));
    expect(series.map((s) => s.name)).toEqual(['CPU', 'Memory']);
  });

  it('adds a dotted GPU line in its own colour, with a break where a point had no reading', () => {
    const series = seriesOf(buildCpuChartOption([withGpu(1000, 30), withGpu(2000, null), withGpu(3000, 50)], palette));
    expect(series.map((s) => s.name)).toEqual(['CPU', 'Memory', 'GPU']);
    expect(series[2]?.data).toEqual([
      [1000, 30],
      [2000, null],
      [3000, 50],
    ]);
    expect(series[2]?.lineStyle.type).toBe('dotted');
    expect(JSON.stringify(buildCpuChartOption([withGpu(1000, 30)], palette))).toContain(palette.gpu);
  });
});
