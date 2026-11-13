import type { RangeOption } from './RangePicker';

const DAY_SECS = 86_400;

/** "All" is simply longer than any history (ten years, the most the backend will look back). */
export const HISTORY_RANGES: RangeOption[] = [
  { label: '24 hours', secs: DAY_SECS },
  { label: '7 days', secs: 7 * DAY_SECS },
  { label: '30 days', secs: 30 * DAY_SECS },
  { label: '90 days', secs: 90 * DAY_SECS },
  { label: '1 year', secs: 365 * DAY_SECS },
  { label: 'All', secs: 3650 * DAY_SECS },
];

export const DEFAULT_HISTORY_RANGE_SECS = 7 * DAY_SECS;
