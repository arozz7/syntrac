import { useMemo } from 'react';
import type { SystemPointView } from '../api/types';
import { buildCpuChartOption, type ChartPalette } from '../charts/cpuChart';
import { Chart } from '../components/Chart';

/** Reads the chart colours from the design tokens, so the theme file stays the single source of truth. */
function readPalette(): ChartPalette {
  const css = getComputedStyle(document.documentElement);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    cpu: token('--chart-1'),
    memory: token('--chart-3'),
    text: token('--color-text'),
    mutedText: token('--color-text-muted'),
    grid: token('--color-border'),
    surface: token('--color-surface'),
  };
}

export function SystemChart({ points }: { points: SystemPointView[] }) {
  const option = useMemo(() => buildCpuChartOption(points, readPalette()), [points]);
  return <Chart option={option} label="CPU and memory use over time" />;
}
