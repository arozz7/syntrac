import type { ProgramHourView } from '../api/types';
import {
  bucketSeries,
  localDayStart,
  type DataBucketBase,
  type DayStart,
  type GapBucket,
  type Granularity,
} from './bucketing';

export interface ProgramBucket extends DataBucketBase {
  hours: number;
  /** In how many of the hours the program was recorded among the heaviest processes. */
  presentHours: number;
  samples: number;
  /** Averaged over every sample, so an hour the program was absent counts as zero (the same rule as the ranking). */
  cpuAvg: number;
  cpuMax: number;
  memoryPeak: number;
  starts: number;
}

function emptyBucket(startMs: number): ProgramBucket {
  return { gap: false, startMs, hours: 0, presentHours: 0, samples: 0, cpuAvg: 0, cpuMax: 0, memoryPeak: 0, starts: 0 };
}

function add(bucket: ProgramBucket, hour: ProgramHourView): void {
  bucket.hours += 1;
  bucket.presentHours += hour.present ? 1 : 0;
  bucket.samples += hour.samples;
  bucket.cpuAvg += hour.cpuAvgPercent * hour.samples;
  bucket.cpuMax = Math.max(bucket.cpuMax, hour.cpuPeakPercent);
  bucket.memoryPeak = Math.max(bucket.memoryPeak, hour.memoryPeakBytes);
  bucket.starts += hour.starts;
}

function finish(bucket: ProgramBucket): ProgramBucket {
  return bucket.samples === 0 ? bucket : { ...bucket, cpuAvg: bucket.cpuAvg / bucket.samples };
}

/** One program's hours grouped by hour or by local day, with gaps where nothing was collected. */
export function bucketProgramHours(
  hours: ProgramHourView[],
  granularity: Granularity,
  dayStart: DayStart = localDayStart,
): Array<ProgramBucket | GapBucket> {
  return bucketSeries(hours, (hour) => hour.hourMs, granularity, dayStart, emptyBucket, add, finish);
}
