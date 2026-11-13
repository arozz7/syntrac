export interface RangeOption {
  label: string;
  secs: number;
}

export const RANGES: RangeOption[] = [
  { label: '15 min', secs: 900 },
  { label: '1 hour', secs: 3600 },
  { label: '6 hours', secs: 21_600 },
  { label: '24 hours', secs: 86_400 },
];

/** A row of toggle buttons for choosing how far back to look. */
export function RangePicker({
  value,
  onChange,
  options = RANGES,
}: {
  value: number;
  onChange: (secs: number) => void;
  options?: RangeOption[];
}) {
  return (
    <div className="range" role="group" aria-label="Time range">
      {options.map((range) => (
        <button
          key={range.secs}
          type="button"
          className="range__button"
          aria-pressed={value === range.secs}
          onClick={() => onChange(range.secs)}
        >
          {range.label}
        </button>
      ))}
    </div>
  );
}
