/** Column sorting for tables: pure, so every table sorts the same way and the rules are tested once. */

export type SortDirection = 'asc' | 'desc';

export interface SortState {
  column: string;
  direction: SortDirection;
}

/** What a row offers for a column. Empty values (null, undefined, NaN) always sort last. */
export type SortValue = number | string | null | undefined;

/** One accessor per sortable column id. */
export type Accessors<T> = Record<string, (row: T) => SortValue>;

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

function isEmpty(value: SortValue): value is null | undefined {
  return value === null || value === undefined || (typeof value === 'number' && Number.isNaN(value));
}

/** Ascending order of two non-empty values: numbers by value, text ignoring case with digits read as numbers. */
function compareValues(a: number | string, b: number | string): number {
  if (typeof a === 'number' && typeof b === 'number') {
    return a - b;
  }
  if (typeof a === 'string' && typeof b === 'string') {
    return collator.compare(a, b);
  }
  return typeof a === 'number' ? -1 : 1;
}

/**
 * A sorted copy of `rows`. No sort (or a column without an accessor) keeps the given order. Equal rows keep their
 * original order in both directions, and empty values come last in both directions.
 */
export function sortRows<T>(rows: readonly T[], state: SortState | null, accessors: Accessors<T>): T[] {
  const accessor = state === null ? undefined : accessors[state.column];
  if (state === null || accessor === undefined) {
    return [...rows];
  }
  const sign = state.direction === 'asc' ? 1 : -1;
  return rows
    .map((row, index) => ({ row, index, value: accessor(row) }))
    .sort((left, right) => {
      const leftEmpty = isEmpty(left.value);
      const rightEmpty = isEmpty(right.value);
      if (leftEmpty || rightEmpty) {
        return leftEmpty === rightEmpty ? left.index - right.index : leftEmpty ? 1 : -1;
      }
      const order = compareValues(left.value as number | string, right.value as number | string) * sign;
      return order !== 0 ? order : left.index - right.index;
    })
    .map((entry) => entry.row);
}

/**
 * What a click on a column header does: a new column starts in its first direction, the same column reverses, and a
 * third click clears the sort (back to the order the data came in).
 */
export function nextSort(current: SortState | null, column: string, firstDirection: SortDirection): SortState | null {
  if (current === null || current.column !== column) {
    return { column, direction: firstDirection };
  }
  if (current.direction === firstDirection) {
    return { column, direction: firstDirection === 'asc' ? 'desc' : 'asc' };
  }
  return null;
}
