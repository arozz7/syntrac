// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { Data } from '../api/types';
import { DataGate } from './DataGate';

afterEach(cleanup);

function gate(result: Data<number> | undefined) {
  render(<DataGate result={result}>{(value) => <p>value is {value}</p>}</DataGate>);
}

describe('DataGate', () => {
  it('shows a loading message until the first result arrives', () => {
    gate(undefined);
    expect(screen.getByText(/Loading/)).toBeTruthy();
  });

  it('renders its children with the data when the result is ok', () => {
    gate({ state: 'ok', data: 7 });
    expect(screen.getByText('value is 7')).toBeTruthy();
  });

  it('explains a missing database and where it comes from', () => {
    gate({ state: 'no_database', message: 'There is no database at X.' });
    expect(screen.getByRole('heading', { name: 'No data yet' })).toBeTruthy();
    expect(screen.getByText('There is no database at X.')).toBeTruthy();
    expect(screen.getByText(/install-service/)).toBeTruthy();
    expect(screen.queryByText(/value is/)).toBeNull();
  });

  it('explains a version mismatch', () => {
    gate({ state: 'schema_mismatch', message: 'The database is schema v1.' });
    expect(screen.getByRole('heading', { name: 'Database version mismatch' })).toBeTruthy();
  });

  it('shows other errors as a plain problem message', () => {
    gate({ state: 'error', message: 'Could not open the database: boom' });
    expect(screen.getByRole('heading', { name: 'Could not read the data' })).toBeTruthy();
    expect(screen.getByText(/boom/)).toBeTruthy();
  });
});
