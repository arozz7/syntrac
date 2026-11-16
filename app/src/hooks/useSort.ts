import { useCallback, useState } from 'react';
import { nextSort, sortRows, type Accessors, type SortDirection, type SortState } from '../sorting';

export interface SortControl {
  state: SortState | null;
  /** Called by a header: see `nextSort`. */
  toggle: (column: string, firstDirection: SortDirection) => void;
  /** The rows in the chosen order (the given order when nothing is chosen). */
  apply: <T>(rows: readonly T[], accessors: Accessors<T>) => T[];
}

/** The sort choice of one table, kept while the view is open. */
export function useSort(initial: SortState | null = null): SortControl {
  const [state, setState] = useState<SortState | null>(initial);
  const toggle = useCallback(
    (column: string, firstDirection: SortDirection) => setState((current) => nextSort(current, column, firstDirection)),
    [],
  );
  const apply = useCallback(
    <T>(rows: readonly T[], accessors: Accessors<T>) => sortRows(rows, state, accessors),
    [state],
  );
  return { state, toggle, apply };
}
