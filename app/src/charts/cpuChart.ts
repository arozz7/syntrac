import type { EChartsOption } from 'echarts';
import type { SystemPointView } from '../api/types';

/** Colours come from the design tokens at runtime; the builder itself stays pure and testable. */
export interface ChartPalette {
  cpu: string;
  memory: string;
  gpu: string;
  text: string;
  mutedText: string;
  grid: string;
  surface: string;
}

/**
 * CPU as a filled line, memory as a DASHED line and the GPU (when there are readings) as a DOTTED line, so the series differ by line style and not only by
 * colour. CPU and memory are shares of the whole machine; the GPU is a share of its busiest engine; all fit the fixed 0-100 axis.
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
      // The GPU line only when some point had a reading: no counters is not an idle GPU. A point without one is a break.
      ...(points.some((p) => p.gpuPercent !== null)
        ? [
            {
              name: 'GPU',
              type: 'line' as const,
              showSymbol: false,
              smooth: false,
              lineStyle: { color: palette.gpu, width: 2, type: 'dotted' as const },
              itemStyle: { color: palette.gpu },
              data: points.map((p) => [p.tsMs, p.gpuPercent]),
            },
          ]
        : []),
    ],
  };
}
