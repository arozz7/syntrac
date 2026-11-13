import { useMemo } from 'react';
import type { SystemPointView } from '../api/types';
import { buildCpuChartOption } from '../charts/cpuChart';
import { readPalette } from '../charts/readPalette';
import { Chart } from '../components/Chart';

export function SystemChart({ points }: { points: SystemPointView[] }) {
  const option = useMemo(() => buildCpuChartOption(points, readPalette()), [points]);
  return <Chart option={option} label="CPU and memory use over time" />;
}
