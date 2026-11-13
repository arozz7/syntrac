import type { ChartPalette } from './cpuChart';

/** Reads the chart colours from the design tokens, so the theme file stays the single source of truth. */
export function readPalette(): ChartPalette {
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
