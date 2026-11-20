import { describe, expect, it } from 'vitest';
import type { DayStart } from './bucketing';
import type { ChartPalette } from './cpuChart';
import { bucketProgramHours } from './programBuckets';
import { buildProgramCpuChartOption, buildProgramGpuChartOption } from './programChart';

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const utc: DayStart = (ms) => Math.floor(ms / DAY) * DAY;
const palette: ChartPalette = {
  cpu: '#111',
  memory: '#222',
  text: '#000',
  mutedText: '#555',
  grid: '#ccc',
  surface: '#fff',
};

const row = (hourMs: number, avg: number) => ({
  hourMs,
  samples: 100,
  present: true,
  cpuAvgPercent: avg,
  cpuPeakPercent: avg * 2,
  memoryPeakBytes: 1,
  gpuAvgPercent: avg / 2,
  gpuPeakPercent: avg * 3,
  gpuMemoryPeakBytes: 7,
  starts: 0,
});

describe('buildProgramCpuChartOption', () => {
  const buckets = bucketProgramHours([row(100 * DAY, 2), row(100 * DAY + 2 * HOUR, 4)], 'hour', utc);
  const series = buildProgramCpuChartOption(buckets, 'hour', palette).series as Array<{
    name: string;
    data: Array<number | null>;
  }>;

  it('draws the average and the peak, with a gap as a null so the line breaks', () => {
    expect(series.map((s) => s.name)).toEqual(['CPU average', 'CPU peak']);
    expect(series[0]!.data).toEqual([2, null, 4]);
    expect(series[1]!.data).toEqual([4, null, 8]);
  });

  it('lets the axis fit a small program instead of fixing it at 100 percent', () => {
    const option = buildProgramCpuChartOption(buckets, 'hour', palette);
    expect((option.yAxis as { max?: number }).max).toBeUndefined();
  });
});

describe('buildProgramGpuChartOption', () => {
  const buckets = bucketProgramHours([row(100 * DAY, 2), row(100 * DAY + 2 * HOUR, 4)], 'hour', utc);
  const option = buildProgramGpuChartOption(buckets, 'hour', palette);
  const series = option.series as Array<{ name: string; data: Array<number | null> }>;

  it('draws the average and the peak share of one engine, with a gap as a null so the line breaks', () => {
    expect(series.map((s) => s.name)).toEqual(['GPU average', 'GPU peak']);
    expect(series[0]!.data).toEqual([1, null, 2]);
    expect(series[1]!.data).toEqual([6, null, 12]);
  });

  it('never goes above one full engine and lets a small user fit the axis from zero', () => {
    const axis = option.yAxis as { min?: number; max?: number };
    expect(axis.min).toBe(0);
    expect(axis.max).toBeUndefined();
  });
});
