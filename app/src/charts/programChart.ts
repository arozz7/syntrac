import type { EChartsOption } from 'echarts';
import type { GapBucket, Granularity } from './bucketing';
import type { ChartPalette } from './cpuChart';
import { frame, line, tooltip, values } from './historyChart';
import type { ProgramBucket } from './programBuckets';

/**
 * One program's CPU: the average (solid, filled) and the peak (dotted), as a share of the whole machine. A program is usually
 * far below 100%, so the axis fits the data instead of being fixed.
 */
export function buildProgramCpuChartOption(
  buckets: Array<ProgramBucket | GapBucket>,
  granularity: Granularity,
  palette: ChartPalette,
): EChartsOption {
  return {
    ...frame(buckets, granularity, palette),
    tooltip: tooltip(palette, (value) => `${value.toFixed(1)}%`),
    yAxis: {
      type: 'value',
      min: 0,
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
    ],
  };
}
