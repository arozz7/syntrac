import { describe, expect, it } from 'vitest';
import { formatAge, formatBytes, formatClock, formatDateTime, formatDuration, formatPercent } from './format';

describe('formatBytes', () => {
  it('uses binary units with one decimal above bytes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB');
    expect(formatBytes(23_342_985)).toBe('22.3 MB');
    expect(formatBytes(5 * 1024 ** 3)).toBe('5.0 GB');
  });

  it('never returns nonsense for bad input', () => {
    expect(formatBytes(-5)).toBe('0 B');
    expect(formatBytes(Number.NaN)).toBe('0 B');
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe('0 B');
  });
});

describe('formatPercent', () => {
  it('shows one decimal', () => {
    expect(formatPercent(12)).toBe('12.0%');
    expect(formatPercent(4.94)).toBe('4.9%');
  });

  it('shows tiny non-zero values as "<0.1%" rather than a misleading 0.0%', () => {
    expect(formatPercent(0.01)).toBe('<0.1%');
    expect(formatPercent(0)).toBe('0.0%');
    expect(formatPercent(Number.NaN)).toBe('0.0%');
  });
});

describe('formatDuration', () => {
  it('picks the two most useful units', () => {
    expect(formatDuration(59)).toBe('59 s');
    expect(formatDuration(61)).toBe('1 min 1 s');
    expect(formatDuration(3661)).toBe('1 h 1 min');
    expect(formatDuration(90_000)).toBe('1 d 1 h');
    expect(formatDuration(-3)).toBe('0 s');
  });
});

describe('formatAge', () => {
  it('reads naturally', () => {
    expect(formatAge(2)).toBe('just now');
    expect(formatAge(12)).toBe('12 s ago');
    expect(formatAge(180)).toBe('3 min ago');
    expect(formatAge(7200)).toBe('2 h ago');
    expect(formatAge(172_800)).toBe('2 d ago');
  });
});

describe('local time formatting', () => {
  const when = new Date(2026, 10, 12, 9, 5, 7).getTime();
  it('formats the clock and the date with zero padding', () => {
    expect(formatClock(when)).toBe('09:05:07');
    expect(formatDateTime(when)).toBe('2026-11-12 09:05');
  });
});
