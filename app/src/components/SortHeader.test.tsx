// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { useSort } from '../hooks/useSort';
import type { Accessors } from '../sorting';
import { SortHeader } from './SortHeader';

afterEach(cleanup);

interface Row {
  name: string;
  size: number;
}

const accessors: Accessors<Row> = { name: (row) => row.name, size: (row) => row.size };
const data: Row[] = [
  { name: 'b', size: 2 },
  { name: 'a', size: 30 },
  { name: 'c', size: 10 },
];

function Example() {
  const sort = useSort();
  const rows = sort.apply(data, accessors);
  return (
    <table>
      <thead>
        <tr>
          <SortHeader sort={sort} column="name" label="Name" />
          <SortHeader sort={sort} column="size" label="Size" firstDirection="desc" className="num" />
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.name}>
            <td>{row.name}</td>
            <td>{row.size}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const order = () =>
  screen
    .getAllByRole('row')
    .slice(1)
    .map((row) => row.firstElementChild?.textContent);
const header = (name: string) => screen.getByRole('columnheader', { name });

describe('SortHeader', () => {
  it('starts unsorted, in the given order, with no column marked', () => {
    render(<Example />);
    expect(order()).toEqual(['b', 'a', 'c']);
    expect(header('Name').getAttribute('aria-sort')).toBe('none');
    expect(header('Size').getAttribute('aria-sort')).toBe('none');
  });

  it('sorts ascending on the first click of a text column, then descending, then clears', async () => {
    render(<Example />);
    const button = screen.getByRole('button', { name: /Name/ });
    await userEvent.click(button);
    expect(order()).toEqual(['a', 'b', 'c']);
    expect(header('Name').getAttribute('aria-sort')).toBe('ascending');
    await userEvent.click(button);
    expect(order()).toEqual(['c', 'b', 'a']);
    expect(header('Name').getAttribute('aria-sort')).toBe('descending');
    await userEvent.click(button);
    expect(order()).toEqual(['b', 'a', 'c']);
    expect(header('Name').getAttribute('aria-sort')).toBe('none');
  });

  it('puts the biggest first on the first click of a number column and marks only that column', async () => {
    render(<Example />);
    await userEvent.click(screen.getByRole('button', { name: /Size/ }));
    expect(order()).toEqual(['a', 'c', 'b']);
    expect(header('Size').getAttribute('aria-sort')).toBe('descending');
    expect(header('Name').getAttribute('aria-sort')).toBe('none');
  });

  it('works from the keyboard', async () => {
    render(<Example />);
    await userEvent.tab();
    await userEvent.keyboard('{Enter}');
    expect(order()).toEqual(['a', 'b', 'c']);
    await userEvent.keyboard(' ');
    expect(order()).toEqual(['c', 'b', 'a']);
  });
});
