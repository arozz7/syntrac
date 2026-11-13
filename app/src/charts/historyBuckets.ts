import type { SystemHourView } from '../api/types';
import {
  bucketSeries,
  granularityFor,
  localDayStart,
  type DataBucketBase,
  type DayStart,
  type GapBucket,
  type Granularity,
} from './bucketing';

// The system history: the stored hourly rows grouped into the buckets the charts draw (see `bucketing.ts` for the rules).
export { granularityFor, localDayStart };
export type { DayStart, GapBucket, Granularity };

export interface DataBucket extends DataBucketBase {
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

/** Hours (oldest first) grouped into buckets by hour or by local day; a bucket with no data between two with data is a gap. */
export function bucketHours(
  hours: SystemHourView[],
  granularity: Granularity,
  dayStart: DayStart = localDayStart,
): Bucket[] {
  return bucketSeries(hours, (hour) => hour.hourMs, granularity, dayStart, emptyBucket, add, finish);
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
