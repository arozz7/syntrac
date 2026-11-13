import type { EChartsOption } from 'echarts';
import type { SystemPointView } from '../api/types';

/** Colours come from the design tokens at runtime; the builder itself stays pure and testable. */
export interface ChartPalette {
  cpu: string;
  memory: string;
  text: string;
  mutedText: string;
  grid: string;
  surface: string;
}

/**
 * CPU as a filled line and memory as a DASHED line, so the two series differ by line style and not only by
 * colour. Both are percentages of the whole machine, hence the fixed 0-100 axis.
 */
export function buildCpuChartOption(points: SystemPointView[], palette: ChartPalette): EChartsOption {
  return {
    animation: false,
    grid: { left: 44, right: 16, top: 28, bottom: 28 },
    legend: { top: 0, right: 8, textStyle: { color: palette.mutedText } },
    tooltip: {
      trigger: 'axis',
      valueFormatter: (value) => `${Number(value).toFixed(1)}%`,
      backgroundColor: palette.surface,
      borderColor: palette.grid,
      textStyle: { color: palette.text },
    },
    xAxis: {
      type: 'time',
      axisLabel: { color: palette.mutedText },
      axisLine: { lineStyle: { color: palette.grid } },
    },
    yAxis: {
      type: 'value',
      min: 0,
      max: 100,
      axisLabel: { color: palette.mutedText, formatter: '{value}%' },
      splitLine: { lineStyle: { color: palette.grid } },
    },
    series: [
      {
        name: 'CPU',
        type: 'line',
        showSymbol: false,
        smooth: false,
        lineStyle: { color: palette.cpu, width: 2 },
        itemStyle: { color: palette.cpu },
        areaStyle: { color: palette.cpu, opacity: 0.15 },
        data: points.map((p) => [p.tsMs, p.cpuPercent]),
      },
      {
        name: 'Memory',
        type: 'line',
        showSymbol: false,
        smooth: false,
        lineStyle: { color: palette.memory, width: 2, type: 'dashed' },
        itemStyle: { color: palette.memory },
        data: points.map((p) => [p.tsMs, p.memoryPercent]),
      },
    ],
  };
}
