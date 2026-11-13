import type { EChartsOption } from 'echarts';
import { formatBytes, formatDateTime, formatDay } from '../format';
import type { ChartPalette } from './cpuChart';
import type { Bucket, DataBucket, Granularity } from './historyBuckets';

// Chart options for the History screens. Pure (colours come in as a palette), so they are unit-tested.
// The x axis is a CATEGORY axis with one entry per bucket: a gap is an explicit null, and spacing stays even.

function labels(buckets: Bucket[], granularity: Granularity): string[] {
  return buckets.map((bucket) => (granularity === 'day' ? formatDay(bucket.startMs) : formatDateTime(bucket.startMs)));
}

function values(buckets: Bucket[], pick: (bucket: DataBucket) => number): Array<number | null> {
  return buckets.map((bucket) => (bucket.gap ? null : pick(bucket)));
}

function frame(buckets: Bucket[], granularity: Granularity, palette: ChartPalette) {
  return {
    animation: false,
    grid: { left: 56, right: 16, top: 28, bottom: 28 },
    legend: { top: 0, right: 8, textStyle: { color: palette.mutedText } },
    xAxis: {
      type: 'category' as const,
      data: labels(buckets, granularity),
      axisLabel: { color: palette.mutedText, hideOverlap: true },
      axisLine: { lineStyle: { color: palette.grid } },
    },
  };
}

function tooltip(palette: ChartPalette, valueFormatter: (value: number) => string) {
  return {
    trigger: 'axis' as const,
    valueFormatter: (value: unknown) => (typeof value === 'number' ? valueFormatter(value) : 'no data'),
    backgroundColor: palette.surface,
    borderColor: palette.grid,
    textStyle: { color: palette.text },
  };
}

/**
 * CPU average as a filled solid line, CPU peak as a dotted line and memory as a dashed line, so the three differ by
 * line style and not only by colour. All are percentages of the whole machine, hence the fixed 0-100 axis.
 */
export function buildLoadChartOption(
  buckets: Bucket[],
  granularity: Granularity,
  palette: ChartPalette,
): EChartsOption {
  const line = (name: string, color: string, type: 'solid' | 'dashed' | 'dotted', width: number) => ({
    name,
    type: 'line' as const,
    showSymbol: false,
    smooth: false,
    lineStyle: { color, width, type },
    itemStyle: { color },
  });
  return {
    ...frame(buckets, granularity, palette),
    tooltip: tooltip(palette, (value) => `${value.toFixed(1)}%`),
    yAxis: {
      type: 'value',
      min: 0,
      max: 100,
      axisLabel: { color: palette.mutedText, formatter: '{value}%' },
      splitLine: { lineStyle: { color: palette.grid } },
    },
    series: [
      {
        ...line('CPU average', palette.cpu, 'solid', 2),
        areaStyle: { color: palette.cpu, opacity: 0.15 },
        data: values(buckets, (b) => b.cpuAvg),
      },
      { ...line('CPU peak', palette.cpu, 'dotted', 1), data: values(buckets, (b) => b.cpuMax) },
      { ...line('Memory average', palette.memory, 'dashed', 2), data: values(buckets, (b) => b.memoryAvg) },
    ],
  };
}

export interface BytesSeries {
  name: string;
  pick: (bucket: DataBucket) => number;
}

/** Bytes per bucket as grouped bars (the first series in the first colour, the second in the second). */
export function buildBytesChartOption(
  buckets: Bucket[],
  granularity: Granularity,
  series: BytesSeries[],
  palette: ChartPalette,
): EChartsOption {
  const colours = [palette.cpu, palette.memory];
  return {
    ...frame(buckets, granularity, palette),
    tooltip: tooltip(palette, formatBytes),
    yAxis: {
      type: 'value',
      min: 0,
      axisLabel: { color: palette.mutedText, formatter: (value: number) => formatBytes(value) },
      splitLine: { lineStyle: { color: palette.grid } },
    },
    series: series.map((entry, index) => ({
      name: entry.name,
      type: 'bar' as const,
      itemStyle: { color: colours[index % colours.length] },
      data: values(buckets, entry.pick),
    })),
  };
}
