import { describe, expect, it } from 'vitest';
import { nextSort, sortRows, type Accessors } from './sorting';

interface Row {
  name: string;
  bytes: number | null;
  seen?: number;
}

const accessors: Accessors<Row> = {
  name: (row) => row.name,
  bytes: (row) => row.bytes,
  seen: (row) => row.seen,
};

const rows: Row[] = [
  { name: 'beta', bytes: 20 },
  { name: 'Alpha', bytes: 5 },
  { name: 'gamma', bytes: null },
  { name: 'delta', bytes: 20 },
  { name: 'alpha2', bytes: 100 },
];

const names = (list: Row[]) => list.map((row) => row.name);

describe('sortRows', () => {
  it('keeps the given order when no sort is chosen, and does not change the input', () => {
    const copy = [...rows];
    expect(names(sortRows(rows, null, accessors))).toEqual(names(rows));
    expect(rows).toEqual(copy);
  });

  it('sorts numbers by value, not as text', () => {
    const sorted = sortRows(rows, { column: 'bytes', direction: 'asc' }, accessors);
    expect(sorted.map((row) => row.bytes)).toEqual([5, 20, 20, 100, null]);
  });

  it('sorts descending with the largest first', () => {
    const sorted = sortRows(rows, { column: 'bytes', direction: 'desc' }, accessors);
    expect(sorted.map((row) => row.bytes)).toEqual([100, 20, 20, 5, null]);
  });

  it('puts empty values last in both directions', () => {
    for (const direction of ['asc', 'desc'] as const) {
      const sorted = sortRows(rows, { column: 'bytes', direction }, accessors);
      expect(sorted[sorted.length - 1]?.name).toBe('gamma');
    }
    const missing = [
      { name: 'x', bytes: 1 },
      { name: 'y', bytes: 2, seen: 3 },
    ];
    expect(names(sortRows(missing, { column: 'seen', direction: 'asc' }, accessors))).toEqual(['y', 'x']);
    expect(names(sortRows(missing, { column: 'seen', direction: 'desc' }, accessors))).toEqual(['y', 'x']);
  });

  it('keeps equal rows in their original order in both directions', () => {
    const asc = sortRows(rows, { column: 'bytes', direction: 'asc' }, accessors);
    const desc = sortRows(rows, { column: 'bytes', direction: 'desc' }, accessors);
    expect(names(asc).filter((name) => name === 'beta' || name === 'delta')).toEqual(['beta', 'delta']);
    expect(names(desc).filter((name) => name === 'beta' || name === 'delta')).toEqual(['beta', 'delta']);
  });

  it('sorts text ignoring case and treating digits as numbers', () => {
    const sorted = sortRows(
      [
        { name: 'file10', bytes: 1 },
        { name: 'File2', bytes: 1 },
        { name: 'apple', bytes: 1 },
      ],
      { column: 'name', direction: 'asc' },
      accessors,
    );
    expect(names(sorted)).toEqual(['apple', 'File2', 'file10']);
  });

  it('treats NaN as empty and ignores a column it has no accessor for', () => {
    const odd: Row[] = [
      { name: 'a', bytes: Number.NaN },
      { name: 'b', bytes: 1 },
    ];
    expect(names(sortRows(odd, { column: 'bytes', direction: 'asc' }, accessors))).toEqual(['b', 'a']);
    expect(names(sortRows(rows, { column: 'nothing', direction: 'asc' }, accessors))).toEqual(names(rows));
  });
});

describe('nextSort', () => {
  it('starts in the column first direction, then reverses, then clears', () => {
    const first = nextSort(null, 'bytes', 'desc');
    expect(first).toEqual({ column: 'bytes', direction: 'desc' });
    const second = nextSort(first, 'bytes', 'desc');
    expect(second).toEqual({ column: 'bytes', direction: 'asc' });
    expect(nextSort(second, 'bytes', 'desc')).toBeNull();
  });

  it('starts again in the first direction when another column is chosen', () => {
    const current = { column: 'bytes', direction: 'asc' } as const;
    expect(nextSort(current, 'name', 'asc')).toEqual({ column: 'name', direction: 'asc' });
  });

  it('clears after a text column went ascending then descending', () => {
    const asc = nextSort(null, 'name', 'asc');
    const desc = nextSort(asc, 'name', 'asc');
    expect(desc).toEqual({ column: 'name', direction: 'desc' });
    expect(nextSort(desc, 'name', 'asc')).toBeNull();
  });
});
