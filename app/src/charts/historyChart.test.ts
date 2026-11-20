import { describe, expect, it } from 'vitest';
import { bucketHours, type DayStart } from './historyBuckets';
import { buildBytesChartOption, buildGpuChartOption, buildLoadChartOption } from './historyChart';
import type { ChartPalette } from './cpuChart';

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const utc: DayStart = (ms) => Math.floor(ms / DAY) * DAY;
const palette: ChartPalette = {
  cpu: '#111',
  memory: '#222',
  gpu: '#333',
  text: '#000',
  mutedText: '#555',
  grid: '#ccc',
  surface: '#fff',
};

function hourRow(hourMs: number, cpuAvg: number) {
  return {
    hourMs,
    samples: 100,
    cpuAvgPercent: cpuAvg,
    cpuMaxPercent: cpuAvg * 2,
    memoryAvgPercent: 50,
    memoryMaxPercent: 60,
    netReceivedBytes: 4096,
    netSentBytes: 1024,
    diskReadBytes: 100,
    diskWriteBytes: 200,
    othersCpuAvgPercent: 0,
    gpuAvgPercent: null,
    gpuMaxPercent: null,
  };
}

// Hours 0 and 2 have data; hour 1 is a gap.
const buckets = bucketHours([hourRow(100 * DAY, 10), hourRow(100 * DAY + 2 * HOUR, 20)], 'hour', utc);

type Series = { name: string; data: Array<number | null>; lineStyle?: { type?: string } };
const seriesOf = (option: ReturnType<typeof buildLoadChartOption>) => option.series as Series[];

describe('buildLoadChartOption', () => {
  it('draws average CPU, peak CPU and memory, with a gap as a null so the line breaks', () => {
    const series = seriesOf(buildLoadChartOption(buckets, 'hour', palette));
    expect(series.map((s) => s.name)).toEqual(['CPU average', 'CPU peak', 'Memory average']);
    expect(series[0]!.data).toEqual([10, null, 20]);
    expect(series[1]!.data).toEqual([20, null, 40]);
    expect(series[2]!.data).toEqual([50, null, 50]);
  });

  it('tells the three lines apart by line style as well as colour', () => {
    const series = seriesOf(buildLoadChartOption(buckets, 'hour', palette));
    const styles = series.map((s) => s.lineStyle?.type ?? 'solid');
    expect(new Set(styles).size).toBe(3);
  });

  it('has one label per bucket, by hour or by day', () => {
    const hourly = buildLoadChartOption(buckets, 'hour', palette);
    expect((hourly.xAxis as { data: string[] }).data).toHaveLength(3);
    const daily = buildLoadChartOption(bucketHours([hourRow(100 * DAY, 10)], 'day', utc), 'day', palette);
    expect((daily.xAxis as { data: string[] }).data).toHaveLength(1);
    expect((daily.xAxis as { data: string[] }).data[0]).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it('keeps the axis at 0-100 percent', () => {
    const option = buildLoadChartOption(buckets, 'hour', palette);
    expect(option.yAxis).toMatchObject({ min: 0, max: 100 });
  });
});

describe('buildBytesChartOption', () => {
  it('draws the two named byte series, with a gap as null', () => {
    const option = buildBytesChartOption(
      buckets,
      'hour',
      [
        { name: 'Received', pick: (b) => b.netReceived },
        { name: 'Sent', pick: (b) => b.netSent },
      ],
      palette,
    );
    const series = option.series as Series[];
    expect(series.map((s) => s.name)).toEqual(['Received', 'Sent']);
    expect(series[0]!.data).toEqual([4096, null, 4096]);
    expect(series[1]!.data).toEqual([1024, null, 1024]);
  });

  it('formats the value axis in human sizes', () => {
    const option = buildBytesChartOption(buckets, 'hour', [{ name: 'Received', pick: (b) => b.netReceived }], palette);
    const formatter = (option.yAxis as { axisLabel: { formatter: (value: number) => string } }).axisLabel.formatter;
    expect(formatter(2048)).toBe('2.0 KB');
  });
});

describe('buildGpuChartOption', () => {
  const withGpu = (hourMs: number, avg: number | null) => ({
    ...hourRow(hourMs, 10),
    gpuAvgPercent: avg,
    gpuMaxPercent: avg === null ? null : avg * 2,
  });
  // Hour 0 has a reading, hour 1 is a gap, hour 2 has data but no GPU counters.
  const gpuBuckets = bucketHours([withGpu(100 * DAY, 20), withGpu(100 * DAY + 2 * HOUR, null)], 'hour', utc);
  const option = buildGpuChartOption(gpuBuckets, 'hour', palette);
  const series = option.series as Array<{ name: string; data: Array<number | null> }>;

  it('draws the average and the peak of the busiest engine; a gap and an hour without counters both break the line', () => {
    expect(series.map((s) => s.name)).toEqual(['GPU average', 'GPU peak']);
    expect(series[0]!.data).toEqual([20, null, null]);
    expect(series[1]!.data).toEqual([40, null, null]);
  });

  it('keeps the axis at 0-100 percent of one engine', () => {
    expect(option.yAxis).toMatchObject({ min: 0, max: 100 });
  });
});
