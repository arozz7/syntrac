import { BarChart, LineChart, SankeyChart } from 'echarts/charts';
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components';
import * as echarts from 'echarts/core';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsOption } from 'echarts';
import { useEffect, useRef } from 'react';

echarts.use([BarChart, LineChart, SankeyChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer]);

/**
 * A thin ECharts wrapper: creates the chart once, resizes with its container, and applies new options. `onSelect` is called with the data of
 * a clicked node or bar (what the option builder put there); `height` overrides the default height in pixels.
 */
export function Chart({
  option,
  label,
  onSelect,
  height,
}: {
  option: EChartsOption;
  label: string;
  onSelect?: (data: unknown) => void;
  height?: number;
}) {
  const selected = useRef(onSelect);
  useEffect(() => {
    selected.current = onSelect;
  });
  const container = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);

  useEffect(() => {
    const element = container.current;
    if (!element) {
      return;
    }
    const instance = echarts.init(element);
    chart.current = instance;
    instance.on('click', (params) => selected.current?.((params as { data?: unknown }).data));
    const observer = new ResizeObserver(() => instance.resize());
    observer.observe(element);
    return () => {
      observer.disconnect();
      instance.dispose();
      chart.current = null;
    };
  }, []);

  useEffect(() => {
    chart.current?.setOption(option, true);
  }, [option]);

  return (
    <div
      ref={container}
      className="chart"
      role="img"
      aria-label={label}
      style={height === undefined ? undefined : { height }}
    />
  );
}
