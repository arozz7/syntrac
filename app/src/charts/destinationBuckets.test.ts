import { describe, expect, it } from 'vitest';
import type { DestinationHourView } from '../api/types';
import type { DayStart } from './bucketing';
import { bucketDestinationHours } from './destinationBuckets';

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
const utc: DayStart = (ms) => Math.floor(ms / DAY) * DAY;

function hour(hourMs: number, overrides: Partial<DestinationHourView> = {}): DestinationHourView {
  return { hourMs, bytesSent: 100, bytesReceived: 1000, flows: 2, ...overrides };
}

describe('bucketDestinationHours', () => {
  it('keeps hours as they are, with a gap where the collector recorded nothing', () => {
    const start = 100 * DAY;
    const buckets = bucketDestinationHours([hour(start), hour(start + 2 * HOUR)], 'hour', utc);
    expect(buckets.map((b) => b.gap)).toEqual([false, true, false]);
  });

  it('adds up bytes and flows over a day, and a quiet hour adds nothing', () => {
    const start = 100 * DAY;
    const buckets = bucketDestinationHours(
      [hour(start), hour(start + HOUR, { bytesSent: 0, bytesReceived: 0, flows: 0 }), hour(start + 2 * HOUR)],
      'day',
      utc,
    );
    const day = buckets[0]!;
    if (day.gap) throw new Error('expected data');
    expect(day.bytesSent).toBe(200);
    expect(day.bytesReceived).toBe(2000);
    expect(day.flows).toBe(4);
  });

  it('returns nothing for no data', () => {
    expect(bucketDestinationHours([], 'day', utc)).toEqual([]);
  });
});
