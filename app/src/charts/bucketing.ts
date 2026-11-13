// Grouping hourly rows into the buckets a history chart draws: by hour, or by LOCAL day. Pure and generic, so the system
// history and a program's history share one set of rules. The time zone is injected (`DayStart`), so 23- and 25-hour
// daylight-saving days are tested without depending on the machine the tests run on.

export const HOUR_MS = 3_600_000;
/** A point 36 hours after a local midnight is always inside the next local day (days are 23 to 25 hours long). */
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

/** A stretch with no data (the collector was down or paused): drawn as a break, never as zero. */
export interface GapBucket {
  gap: true;
  startMs: number;
}

/** What a data bucket must be: not a gap, and starting somewhere. */
export interface DataBucketBase {
  gap: false;
  startMs: number;
}

function nextStart(startMs: number, granularity: Granularity, dayStart: DayStart): number {
  return granularity === 'hour' ? startMs + HOUR_MS : dayStart(startMs + INTO_NEXT_DAY_MS);
}

/**
 * Hours grouped into buckets. `make` starts an empty bucket, `add` folds one hour into it, `finish` turns the accumulated sums
 * into the bucket's final values. Between the first and last bucket with data, a bucket with none is a gap; nothing is added
 * before the first or after the last, so a short history stays short.
 */
export function bucketSeries<H, B extends DataBucketBase>(
  hours: H[],
  hourMs: (hour: H) => number,
  granularity: Granularity,
  dayStart: DayStart,
  make: (startMs: number) => B,
  add: (bucket: B, hour: H) => void,
  finish: (bucket: B) => B,
): Array<B | GapBucket> {
  const byStart = new Map<number, B>();
  for (const hour of [...hours].sort((a, b) => hourMs(a) - hourMs(b))) {
    const start = granularity === 'hour' ? hourMs(hour) : dayStart(hourMs(hour));
    const bucket = byStart.get(start) ?? make(start);
    add(bucket, hour);
    byStart.set(start, bucket);
  }
  const starts = [...byStart.keys()];
  const first = starts[0];
  const last = starts.at(-1);
  if (first === undefined || last === undefined) {
    return [];
  }
  const buckets: Array<B | GapBucket> = [];
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
