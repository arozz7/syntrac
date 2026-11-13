import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { colourDistance, contrastRatio } from './contrast';

const WCAG_AA_TEXT = 4.5;
const MIN_TIER_DISTANCE = 60;

const WCAG_AA_GRAPHICS = 3;

/** Reads `--name: #rrggbb;` and `--name: var(--other);` declarations, resolving references. */
function loadTokens(): Record<string, string> {
  const css = readFileSync(new URL('./tokens.css', import.meta.url), 'utf8');
  const declared = new Map<string, string>();
  for (const [, name, value] of css.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-f]{6}|var\(--[a-z0-9-]+\))\s*;/gi)) {
    if (name && value) {
      declared.set(name, value);
    }
  }

  const resolve = (name: string, seen: string[] = []): string => {
    const value = declared.get(name);
    if (!value || seen.includes(name)) {
      throw new Error(`Cannot resolve design token --${name}`);
    }
    const reference = /^var\(--([a-z0-9-]+)\)$/i.exec(value);
    return reference?.[1] ? resolve(reference[1], [...seen, name]) : value;
  };

  return Object.fromEntries([...declared.keys()].map((name) => [name, resolve(name)]));
}

const tokens = loadTokens();

function token(name: string): string {
  const value = tokens[name];
  if (!value) {
    throw new Error(`Missing design token --${name}`);
  }
  return value;
}

describe('design tokens', () => {
  it('defines the full palette', () => {
    expect(Object.keys(tokens).length).toBeGreaterThanOrEqual(20);
  });

  const textPairs: Array<[string, string]> = [
    ['color-text', 'color-bg'],
    ['color-text', 'color-surface'],
    ['color-text', 'color-surface-alt'],
    ['color-text-muted', 'color-bg'],
    ['color-text-muted', 'color-surface'],
    ['color-text-muted', 'color-surface-alt'],
    ['color-on-primary', 'color-primary'],
    ['color-on-accent', 'color-accent'],
    ['severity-unusual-fg', 'severity-unusual-bg'],
    ['severity-suspicious-fg', 'severity-suspicious-bg'],
    ['severity-confirmed-fg', 'severity-confirmed-bg'],
    ['status-running-fg', 'status-running-bg'],
    ['status-stale-fg', 'status-stale-bg'],
    ['status-offline-fg', 'status-offline-bg'],
    ['status-unknown-fg', 'status-unknown-bg'],
  ];

  it.each(textPairs)('%s on %s meets WCAG AA for text', (foreground, background) => {
    expect(contrastRatio(token(foreground), token(background))).toBeGreaterThanOrEqual(WCAG_AA_TEXT);
  });

  it('keeps the three severity tiers visually distinct', () => {
    const tiers = ['unusual', 'suspicious', 'confirmed'].map((tier) => token(`severity-${tier}-bg`));
    for (let i = 0; i < tiers.length; i++) {
      for (let j = i + 1; j < tiers.length; j++) {
        expect(colourDistance(tiers[i] ?? '', tiers[j] ?? '')).toBeGreaterThanOrEqual(MIN_TIER_DISTANCE);
      }
    }
  });

  it('keeps severity chips distinguishable from the page surfaces', () => {
    for (const tier of ['unusual', 'suspicious', 'confirmed']) {
      expect(colourDistance(token(`severity-${tier}-bg`), token('color-surface'))).toBeGreaterThanOrEqual(30);
    }
  });
});

describe('collector health colours', () => {
  const states = ['running', 'stale', 'offline', 'unknown'].map((state) => token(`status-${state}-bg`));

  it('keep the four states visually distinct from each other', () => {
    for (let i = 0; i < states.length; i++) {
      for (let j = i + 1; j < states.length; j++) {
        expect(colourDistance(states[i] ?? '', states[j] ?? '')).toBeGreaterThanOrEqual(40);
      }
    }
  });

  it('stay visibly separate from the card surface they sit on', () => {
    for (const state of states) {
      expect(colourDistance(state, token('color-surface'))).toBeGreaterThanOrEqual(30);
    }
  });
});

describe('brand and chart colours', () => {
  const MIN_BRAND_VS_ALERT_DISTANCE = 40;
  const alertBackgrounds = ['unusual', 'suspicious', 'confirmed'].map((tier) => `severity-${tier}-bg`);
  const nonAlertColours = [
    'color-primary',
    'color-accent',
    ...Object.keys(tokens).filter((n) => n.startsWith('chart-')),
  ];

  const chartColours = Object.keys(tokens).filter((name) => name.startsWith('chart-'));

  it.each(chartColours)('%s has 3:1 contrast against card surfaces (graphics)', (name) => {
    expect(contrastRatio(token(name), token('color-surface'))).toBeGreaterThanOrEqual(WCAG_AA_GRAPHICS);
  });

  it('has chart colours to check', () => {
    expect(nonAlertColours.length).toBeGreaterThanOrEqual(8);
  });

  const pairs = nonAlertColours.flatMap((colour) => alertBackgrounds.map((alert): [string, string] => [colour, alert]));

  it.each(pairs)('%s is not mistakable for %s', (colour, alert) => {
    expect(colourDistance(token(colour), token(alert))).toBeGreaterThanOrEqual(MIN_BRAND_VS_ALERT_DISTANCE);
  });
});

describe('contrast helpers', () => {
  it('computes the known black/white ratio', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 5);
  });

  it('rejects malformed colours', () => {
    expect(() => contrastRatio('red', '#ffffff')).toThrow();
  });
});
