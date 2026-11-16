import type { SortControl } from '../hooks/useSort';
import type { SortDirection } from '../sorting';

interface SortHeaderProps {
  sort: SortControl;
  /** The id the table's accessors use for this column. */
  column: string;
  label: string;
  /** Text columns start ascending (the default); number and date columns usually start descending (biggest, newest). */
  firstDirection?: SortDirection;
  className?: string;
}

/** A column header that sorts its table: a real button, so the keyboard and screen readers work. */
export function SortHeader({ sort, column, label, firstDirection = 'asc', className }: SortHeaderProps) {
  const active = sort.state?.column === column ? sort.state.direction : null;
  const ariaSort = active === null ? 'none' : active === 'asc' ? 'ascending' : 'descending';
  return (
    <th className={className} aria-sort={ariaSort}>
      <button type="button" className="sortbutton" onClick={() => sort.toggle(column, firstDirection)}>
        {label}
        <span className="sortbutton__arrow" aria-hidden="true">
          {active === 'asc' ? '▲' : active === 'desc' ? '▼' : '↕'}
        </span>
      </button>
    </th>
  );
}
