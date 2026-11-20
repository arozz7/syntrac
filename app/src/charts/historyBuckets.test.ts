import { describe, expect, it } from 'vitest';
import type { SystemHourView } from '../api/types';
import { bucketHours, granularityFor, summarise, type DayStart } from './historyBuckets';

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** A time zone five hours behind UTC, with no daylight saving: a day starts at 05:00 UTC. */
const utcMinus5: DayStart = (ms) => Math.floor((ms - 5 * HOUR) / DAY) * DAY + 5 * HOUR;

function hour(startMs: number, overrides: Partial<SystemHourView> = {}): SystemHourView {
  return {
    hourMs: startMs,
    samples: 100,
    cpuAvgPercent: 10,
    cpuMaxPercent: 30,
    memoryAvgPercent: 50,
    memoryMaxPercent: 60,
    netReceivedBytes: 1000,
    netSentBytes: 100,
    diskReadBytes: 10,
    diskWriteBytes: 20,
    othersCpuAvgPercent: 1,
    gpuAvgPercent: null,
    gpuMaxPercent: null,
    ...overrides,
  };
}

describe('granularityFor', () => {
  it('shows hours up to a week and days beyond it', () => {
    expect(granularityFor(24 * 3600)).toBe('hour');
    expect(granularityFor(7 * 24 * 3600)).toBe('hour');
    expect(granularityFor(30 * 24 * 3600)).toBe('day');
    expect(granularityFor(365 * 24 * 3600)).toBe('day');
  });
});

describe('bucketHours by hour', () => {
  it('keeps one bucket per hour and leaves a gap for a missing hour, never a zero', () => {
    const start = 100 * DAY;
    const buckets = bucketHours([hour(start), hour(start + 3 * HOUR)], 'hour', utcMinus5);

    expect(buckets.map((b) => b.startMs)).toEqual([start, start + HOUR, start + 2 * HOUR, start + 3 * HOUR]);
    expect(buckets.map((b) => b.gap)).toEqual([false, true, true, false]);
  });

  it('does not pad before the first or after the last hour of data', () => {
    const start = 100 * DAY;
    const buckets = bucketHours([hour(start), hour(start + HOUR)], 'hour', utcMinus5);
    expect(buckets).toHaveLength(2);
  });

  it('returns nothing for no data', () => {
    expect(bucketHours([], 'hour', utcMinus5)).toEqual([]);
    expect(bucketHours([], 'day', utcMinus5)).toEqual([]);
  });
});

describe('bucketHours by local day', () => {
  it('groups hours into the local day, weighting averages by the samples behind them', () => {
    const dayStart = 100 * DAY + 5 * HOUR;
    const buckets = bucketHours(
      [
        hour(dayStart, {
          samples: 300,
          cpuAvgPercent: 10,
          cpuMaxPercent: 40,
          memoryAvgPercent: 40,
          memoryMaxPercent: 45,
        }),
        hour(dayStart + HOUR, {
          samples: 100,
          cpuAvgPercent: 30,
          cpuMaxPercent: 90,
          memoryAvgPercent: 60,
          memoryMaxPercent: 70,
        }),
      ],
      'day',
      utcMinus5,
    );

    expect(buckets).toHaveLength(1);
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect(day.startMs).toBe(dayStart);
    expect(day.cpuAvg).toBeCloseTo(15, 9); // (10*300 + 30*100) / 400
    expect(day.cpuMax).toBe(90);
    expect(day.memoryAvg).toBeCloseTo(45, 9);
    expect(day.memoryMax).toBe(70);
    expect(day.netReceived).toBe(2000);
    expect(day.netSent).toBe(200);
    expect(day.diskRead).toBe(20);
    expect(day.diskWrite).toBe(40);
    expect(day.hours).toBe(2);
    expect(day.samples).toBe(400);
  });

  it('puts hours either side of LOCAL midnight into different days', () => {
    const midnight = 100 * DAY + 5 * HOUR; // 00:00 local
    const buckets = bucketHours([hour(midnight - HOUR), hour(midnight)], 'day', utcMinus5);
    expect(buckets.map((b) => b.startMs)).toEqual([midnight - DAY, midnight]);
  });

  it('shows a day with no data between two days with data as a gap', () => {
    const day0 = 100 * DAY + 5 * HOUR;
    const buckets = bucketHours([hour(day0), hour(day0 + 3 * DAY)], 'day', utcMinus5);
    expect(buckets.map((b) => b.gap)).toEqual([false, true, true, false]);
    expect(buckets.map((b) => b.startMs)).toEqual([day0, day0 + DAY, day0 + 2 * DAY, day0 + 3 * DAY]);
  });

  it('copes with a 23-hour day (daylight saving): no duplicate or invented day', () => {
    // Day boundaries of a zone whose first day is only 23 hours long.
    const starts = [0, 23 * HOUR, 47 * HOUR, 71 * HOUR];
    const dayStart: DayStart = (ms) => starts.filter((s) => s <= ms).at(-1) ?? starts[0]!;

    const buckets = bucketHours([hour(0), hour(30 * HOUR), hour(50 * HOUR)], 'day', dayStart);

    expect(buckets.map((b) => b.startMs)).toEqual([0, 23 * HOUR, 47 * HOUR]);
    expect(buckets.every((b) => !b.gap)).toBe(true);
  });

  it('stops instead of hanging when a day-start rule does not advance', () => {
    const stuck: DayStart = () => 0;
    const buckets = bucketHours([hour(0), hour(10 * DAY)], 'day', stuck);
    expect(buckets.length).toBeGreaterThan(0);
  });

  it('copes with a 25-hour day', () => {
    const starts = [0, 25 * HOUR, 49 * HOUR, 73 * HOUR];
    const dayStart: DayStart = (ms) => starts.filter((s) => s <= ms).at(-1) ?? starts[0]!;

    const buckets = bucketHours([hour(0), hour(24 * HOUR), hour(75 * HOUR)], 'day', dayStart);

    // Hours 0 and 24 are both in the long first day; day 2 (49h) has no data, so it is a gap.
    expect(buckets.map((b) => b.startMs)).toEqual([0, 25 * HOUR, 49 * HOUR, 73 * HOUR]);
    expect(buckets.map((b) => b.gap)).toEqual([false, true, true, false]);
    const first = buckets[0]!;
    if (first.gap) throw new Error('expected data');
    expect(first.hours).toBe(2);
  });
});

describe('summarise', () => {
  it('weights the average by samples and ignores gaps', () => {
    const start = 100 * DAY;
    const buckets = bucketHours(
      [
        hour(start, { samples: 100, cpuAvgPercent: 10, cpuMaxPercent: 20 }),
        hour(start + 2 * HOUR, { samples: 300, cpuAvgPercent: 30, cpuMaxPercent: 80 }),
      ],
      'hour',
      utcMinus5,
    );
    const summary = summarise(buckets)!;
    expect(summary.cpuAvg).toBeCloseTo(25, 9);
    expect(summary.cpuMax).toBe(80);
    expect(summary.netReceived).toBe(2000);
    expect(summary.hours).toBe(2);
  });

  it('has nothing to say about no data', () => {
    expect(summarise([])).toBeNull();
  });
});

describe('the GPU of the machine', () => {
  it('has no GPU figures at all when no hour had a reading (no counters is not an idle GPU)', () => {
    const buckets = bucketHours([hour(100 * DAY), hour(100 * DAY + HOUR)], 'day', utcMinus5);
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect([day.gpuAvg, day.gpuMax]).toEqual([null, null]);
    expect(summarise(buckets)).toMatchObject({ gpuAvg: null, gpuMax: null });
  });

  it('averages over the hours that had a reading, weighted by their samples, and keeps the highest peak', () => {
    const start = 100 * DAY;
    const buckets = bucketHours(
      [
        hour(start, { samples: 300, gpuAvgPercent: 40, gpuMaxPercent: 70 }),
        hour(start + HOUR, { samples: 100, gpuAvgPercent: 0, gpuMaxPercent: 10 }),
        hour(start + 2 * HOUR, { samples: 500 }), // no counters this hour: left out of the average, not counted as zero
      ],
      'day',
      utcMinus5,
    );
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect(day.gpuAvg).toBeCloseTo(30, 9); // (40*300 + 0*100) / 400
    expect(day.gpuMax).toBe(70);
    expect(summarise(buckets)).toMatchObject({ gpuMax: 70 });
    expect(summarise(buckets)!.gpuAvg).toBeCloseTo(30, 9);
  });
});
