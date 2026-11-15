import type { DestinationHourView } from '../api/types';
import {
  bucketSeries,
  localDayStart,
  type DataBucketBase,
  type DayStart,
  type GapBucket,
  type Granularity,
} from './bucketing';

export interface DestinationBucket extends DataBucketBase {
  bytesSent: number;
  bytesReceived: number;
  flows: number;
}

function emptyBucket(startMs: number): DestinationBucket {
  return { gap: false, startMs, bytesSent: 0, bytesReceived: 0, flows: 0 };
}

function add(bucket: DestinationBucket, hour: DestinationHourView): void {
  bucket.bytesSent += hour.bytesSent;
  bucket.bytesReceived += hour.bytesReceived;
  bucket.flows += hour.flows;
}

/** One destination's hours grouped by hour or by local day, with gaps where nothing was collected. */
export function bucketDestinationHours(
  hours: DestinationHourView[],
  granularity: Granularity,
  dayStart: DayStart = localDayStart,
): Array<DestinationBucket | GapBucket> {
  return bucketSeries(
    hours,
    (hour) => hour.hourMs,
    granularity,
    dayStart,
    emptyBucket,
    add,
    (bucket) => bucket,
  );
}
