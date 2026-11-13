import type { SystemHourView } from '../api/types';

// Groups the stored hourly rows into the buckets the history charts draw. Pure: the time zone is injected, so daylight
// saving and midnight are tested without depending on the machine the tests run on.

const HOUR_MS = 3_600_000;
/// A point 36 hours after a local midnight is always inside the next local day (days are 23 to 25 hours long).
const INTO_NEXT_DAY_MS = 36 * HOUR_MS;
const WEEK_SECS = 7 * 24 * 3600;

export type Granularity = 'hour' | 'day';

/** The start (epoch ms) of the local day that contains `ms`. */
export type DayStart = (ms: number) => number;

export const localDayStart: DayStart = (ms) => {
  const date = new Date(ms);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
};

/** Hours up to a week, local-time days beyond it. */
export function granularityFor(rangeSecs: number): Granularity {
  return rangeSecs <= WEEK_SECS ? 'hour' : 'day';
}

export interface DataBucket {
  gap: false;
  startMs: number;
  /** Hours of data in the bucket (a day with the collector down for part of it has fewer than 24). */
  hours: number;
  samples: number;
  cpuAvg: number;
  cpuMax: number;
  memoryAvg: number;
  memoryMax: number;
  netReceived: number;
  netSent: number;
  diskRead: number;
  diskWrite: number;
}

/** A stretch with no data (the collector was down or paused): drawn as a break, never as zero. */
export interface GapBucket {
  gap: true;
  startMs: number;
}

export type Bucket = DataBucket | GapBucket;

function emptyBucket(startMs: number): DataBucket {
  return {
    gap: false,
    startMs,
    hours: 0,
    samples: 0,
    cpuAvg: 0,
    cpuMax: 0,
    memoryAvg: 0,
    memoryMax: 0,
    netReceived: 0,
    netSent: 0,
    diskRead: 0,
    diskWrite: 0,
  };
}

/** Adds an hour in. Averages are kept as sums weighted by samples until `finish` divides them. */
function add(bucket: DataBucket, hour: SystemHourView): void {
  bucket.hours += 1;
  bucket.samples += hour.samples;
  bucket.cpuAvg += hour.cpuAvgPercent * hour.samples;
  bucket.memoryAvg += hour.memoryAvgPercent * hour.samples;
  bucket.cpuMax = Math.max(bucket.cpuMax, hour.cpuMaxPercent);
  bucket.memoryMax = Math.max(bucket.memoryMax, hour.memoryMaxPercent);
  bucket.netReceived += hour.netReceivedBytes;
  bucket.netSent += hour.netSentBytes;
  bucket.diskRead += hour.diskReadBytes;
  bucket.diskWrite += hour.diskWriteBytes;
}

function finish(bucket: DataBucket): DataBucket {
  if (bucket.samples === 0) {
    return bucket;
  }
  return { ...bucket, cpuAvg: bucket.cpuAvg / bucket.samples, memoryAvg: bucket.memoryAvg / bucket.samples };
}

function nextStart(startMs: number, granularity: Granularity, dayStart: DayStart): number {
  return granularity === 'hour' ? startMs + HOUR_MS : dayStart(startMs + INTO_NEXT_DAY_MS);
}

/**
 * Hours (oldest first) grouped into buckets. Between the first and last bucket with data, a bucket with none is a gap.
 * Nothing is added before the first or after the last: a short history stays short.
 */
export function bucketHours(
  hours: SystemHourView[],
  granularity: Granularity,
  dayStart: DayStart = localDayStart,
): Bucket[] {
  const byStart = new Map<number, DataBucket>();
  for (const hour of [...hours].sort((a, b) => a.hourMs - b.hourMs)) {
    const start = granularity === 'hour' ? hour.hourMs : dayStart(hour.hourMs);
    const bucket = byStart.get(start) ?? emptyBucket(start);
    add(bucket, hour);
    byStart.set(start, bucket);
  }
  const starts = [...byStart.keys()];
  const first = starts[0];
  const last = starts.at(-1);
  if (first === undefined || last === undefined) {
    return [];
  }
  const buckets: Bucket[] = [];
  let start = first;
  while (start <= last) {
    const bucket = byStart.get(start);
    buckets.push(bucket ? finish(bucket) : { gap: true, startMs: start });
    const next = nextStart(start, granularity, dayStart);
    if (next <= start) {
      break; // a day-start rule that does not advance must never hang the screen
    }
    start = next;
  }
  return buckets;
}

export interface Summary {
  hours: number;
  samples: number;
  cpuAvg: number;
  cpuMax: number;
  memoryAvg: number;
  memoryMax: number;
  netReceived: number;
  netSent: number;
  diskRead: number;
  diskWrite: number;
}

/** The whole range in one line of facts. `null` when there is no data at all. */
export function summarise(buckets: Bucket[]): Summary | null {
  const data = buckets.filter((bucket): bucket is DataBucket => !bucket.gap);
  if (data.length === 0) {
    return null;
  }
  const samples = data.reduce((total, b) => total + b.samples, 0);
  const weighted = (pick: (b: DataBucket) => number) =>
    samples === 0 ? 0 : data.reduce((total, b) => total + pick(b) * b.samples, 0) / samples;
  const sum = (pick: (b: DataBucket) => number) => data.reduce((total, b) => total + pick(b), 0);
  return {
    hours: sum((b) => b.hours),
    samples,
    cpuAvg: weighted((b) => b.cpuAvg),
    cpuMax: Math.max(...data.map((b) => b.cpuMax)),
    memoryAvg: weighted((b) => b.memoryAvg),
    memoryMax: Math.max(...data.map((b) => b.memoryMax)),
    netReceived: sum((b) => b.netReceived),
    netSent: sum((b) => b.netSent),
    diskRead: sum((b) => b.diskRead),
    diskWrite: sum((b) => b.diskWrite),
  };
}
