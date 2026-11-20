import { describe, expect, it } from 'vitest';
import type { ProgramHourView } from '../api/types';
import type { DayStart } from './bucketing';
import { bucketProgramHours } from './programBuckets';

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const utc: DayStart = (ms) => Math.floor(ms / DAY) * DAY;

function hour(hourMs: number, overrides: Partial<ProgramHourView> = {}): ProgramHourView {
  return {
    hourMs,
    samples: 100,
    present: true,
    cpuAvgPercent: 10,
    cpuPeakPercent: 30,
    memoryPeakBytes: 1000,
    gpuAvgPercent: 0,
    gpuPeakPercent: 0,
    gpuMemoryPeakBytes: 0,
    starts: 1,
    ...overrides,
  };
}

describe('bucketProgramHours', () => {
  it('keeps hours as they are, with a gap where the collector recorded nothing', () => {
    const start = 100 * DAY;
    const buckets = bucketProgramHours([hour(start), hour(start + 2 * HOUR)], 'hour', utc);
    expect(buckets.map((b) => b.gap)).toEqual([false, true, false]);
  });

  it('counts an hour in which the program was absent as zero, weighted like any other hour', () => {
    const start = 100 * DAY;
    const buckets = bucketProgramHours(
      [
        hour(start, { samples: 300, cpuAvgPercent: 20 }),
        hour(start + HOUR, {
          samples: 100,
          present: false,
          cpuAvgPercent: 0,
          cpuPeakPercent: 0,
          memoryPeakBytes: 0,
          starts: 0,
        }),
      ],
      'day',
      utc,
    );
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect(day.cpuAvg).toBeCloseTo(15, 9); // (20*300 + 0*100) / 400
    expect(day.cpuMax).toBe(30);
    expect(day.memoryPeak).toBe(1000);
    expect(day.starts).toBe(1);
    expect(day.hours).toBe(2);
    expect(day.presentHours).toBe(1);
  });

  it('sums starts and takes the highest memory peak over the day', () => {
    const start = 100 * DAY;
    const buckets = bucketProgramHours(
      [hour(start, { starts: 2, memoryPeakBytes: 500 }), hour(start + HOUR, { starts: 3, memoryPeakBytes: 900 })],
      'day',
      utc,
    );
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect(day.starts).toBe(5);
    expect(day.memoryPeak).toBe(900);
  });

  it('returns nothing for no data', () => {
    expect(bucketProgramHours([], 'day', utc)).toEqual([]);
  });
});

describe('bucketProgramHours: the GPU', () => {
  it('weights the GPU average by samples like the CPU, and keeps the highest peak and the highest memory', () => {
    const start = 100 * DAY;
    const buckets = bucketProgramHours(
      [
        hour(start, { samples: 300, gpuAvgPercent: 40, gpuPeakPercent: 55, gpuMemoryPeakBytes: 2000 }),
        hour(start + HOUR, { samples: 100, gpuAvgPercent: 0, gpuPeakPercent: 10, gpuMemoryPeakBytes: 9000 }),
      ],
      'day',
      utc,
    );
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect(day.gpuAvg).toBeCloseTo(30, 9); // (40*300 + 0*100) / 400
    expect(day.gpuMax).toBe(55);
    expect(day.gpuMemoryPeak).toBe(9000);
  });
});
