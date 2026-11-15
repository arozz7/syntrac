// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import {
  destinationHistoryView,
  destinationRow,
  destinationsView,
  makeFakeApi,
  newDestinationRow,
  newDestinationsView,
} from '../test/fakeApi';
import { NetworkHistory } from './NetworkHistory';

// jsdom has no canvas; the charts are exercised through their pure option builders.
vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

const WEEK = 7 * 24 * 3600;

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <NetworkHistory />
    </ApiProvider>,
  );
  return api;
}

describe('NetworkHistory: top destinations', () => {
  it('lists destinations with the name that was asked for, in human units', async () => {
    show();
    const row = (await screen.findByRole('button', { name: '142.251.40.238' })).closest('tr')!;
    expect(within(row).getByText('calendar.google.com (2 other names)')).toBeTruthy();
    expect(within(row).getByText('5.0 MB')).toBeTruthy(); // received
    expect(within(row).getByText('2.0 KB')).toBeTruthy(); // sent
    expect(within(row).getByText('40')).toBeTruthy(); // flows
  });

  it('says plainly when no name was recorded for an address', async () => {
    show();
    const row = (await screen.findByRole('button', { name: '203.0.113.50' })).closest('tr')!;
    expect(within(row).getByText('No name seen')).toBeTruthy();
  });

  it('opens on the last 7 days by bytes, on addresses on the internet only', async () => {
    const api = show();
    await screen.findByRole('button', { name: '142.251.40.238' });
    expect(api.topDestinations).toHaveBeenLastCalledWith(WEEK, 'bytes', undefined, false);
    expect(screen.getByRole('button', { name: '7 days' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'Bytes' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('ranks again when another order or range is chosen', async () => {
    const api = show();
    await screen.findByRole('button', { name: '142.251.40.238' });

    await userEvent.click(screen.getByRole('button', { name: 'Programs' }));
    await waitFor(() => expect(api.topDestinations).toHaveBeenLastCalledWith(WEEK, 'programs', undefined, false));

    await userEvent.click(screen.getByRole('button', { name: '30 days' }));
    await waitFor(() =>
      expect(api.topDestinations).toHaveBeenLastCalledWith(30 * 24 * 3600, 'programs', undefined, false),
    );
  });

  it('asks for every address, including this network and multicast, when switched on', async () => {
    const api = show();
    await screen.findByRole('button', { name: '142.251.40.238' });

    await userEvent.click(screen.getByRole('button', { name: /include this network/i }));

    await waitFor(() => expect(api.topDestinations).toHaveBeenLastCalledWith(WEEK, 'bytes', undefined, true));
    expect(api.newDestinations).toHaveBeenLastCalledWith(WEEK, undefined, true);
  });

  it('labels an address that is not on the internet', async () => {
    show(
      makeFakeApi({
        topDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: destinationsView({
            destinations: [destinationRow({ addr: '192.168.1.1', name: null, otherNames: 0, class: 'private' })],
          }),
        })),
      }),
    );
    const row = (await screen.findByRole('button', { name: '192.168.1.1' })).closest('tr')!;
    expect(within(row).getByText('This network')).toBeTruthy();
  });

  it('says what a name means and does not mean', async () => {
    show();
    await screen.findByRole('button', { name: '142.251.40.238' });
    expect(screen.getByText(/name a program asked for/i)).toBeTruthy();
    expect(screen.getByText(/not who owns the address/i)).toBeTruthy();
  });

  it('says where the history begins when the range reaches back further than it', async () => {
    show(
      makeFakeApi({
        topDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: destinationsView({ fromMs: 0, historyStartsMs: 129_600_000 }),
        })),
      }),
    );
    expect(await screen.findByText(/history begins 1970-01-02/i)).toBeTruthy();
  });

  it('explains an empty ranking instead of drawing an empty table', async () => {
    show(
      makeFakeApi({
        topDestinations: vi.fn(async () => ({ state: 'ok' as const, data: destinationsView({ destinations: [] }) })),
      }),
    );
    expect(await screen.findByText(/no destinations/i)).toBeTruthy();
  });

  it('shows the gate message instead of numbers when there is no database', async () => {
    show(
      makeFakeApi({
        topDestinations: vi.fn(async () => ({ state: 'no_database' as const, message: 'There is no database.' })),
      }),
    );
    expect(await screen.findAllByRole('heading', { name: 'No data yet' })).not.toHaveLength(0);
  });
});

describe('NetworkHistory: new destinations', () => {
  it('lists what is new with the program that talked to it first', async () => {
    show();
    const row = (await screen.findByRole('button', { name: '198.51.100.7' })).closest('tr')!;
    expect(within(row).getByText('later.example.org')).toBeTruthy();
    expect(within(row).getByText('code.exe')).toBeTruthy();
  });

  it('opens on the last 7 days of news', async () => {
    const api = show();
    await screen.findByRole('button', { name: '198.51.100.7' });
    expect(api.newDestinations).toHaveBeenLastCalledWith(WEEK, undefined, false);
  });

  it('asks again when another window is chosen', async () => {
    const api = show();
    await screen.findByRole('button', { name: '198.51.100.7' });

    await userEvent.click(screen.getByRole('button', { name: 'Last day' }));

    await waitFor(() => expect(api.newDestinations).toHaveBeenLastCalledWith(24 * 3600, undefined, false));
  });

  it('explains that the first day of history is the baseline when that limits the window', async () => {
    show();
    expect(await screen.findByText(/first day of history/i)).toBeTruthy();
  });

  it('says plainly when nothing is new', async () => {
    show(
      makeFakeApi({
        newDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: newDestinationsView({ destinations: [], sinceMs: null, baselineUntilMs: null }),
        })),
      }),
    );
    expect(await screen.findByText(/nothing new/i)).toBeTruthy();
  });

  it('shows an unidentified first program as such', async () => {
    show(
      makeFakeApi({
        newDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: newDestinationsView({
            destinations: [newDestinationRow({ firstProgram: null, firstProgramPath: null })],
          }),
        })),
      }),
    );
    const row = (await screen.findByRole('button', { name: '198.51.100.7' })).closest('tr')!;
    expect(within(row).getByText('Unknown program')).toBeTruthy();
  });
});

describe('NetworkHistory: one destination', () => {
  it('opens a destination from the ranking', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: '142.251.40.238' }));

    expect(await screen.findByLabelText('Traffic to this address over time')).toBeTruthy();
    expect(api.destinationHistory).toHaveBeenLastCalledWith(WEEK, '142.251.40.238');
    expect(screen.getByRole('heading', { name: '142.251.40.238' })).toBeTruthy();
  });

  it('opens a destination from the list of new ones', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: '198.51.100.7' }));

    await screen.findByLabelText('Traffic to this address over time');
    expect(api.destinationHistory).toHaveBeenLastCalledWith(WEEK, '198.51.100.7');
  });

  it('lists every name asked for and the programs that talked to it', async () => {
    show();
    await userEvent.click(await screen.findByRole('button', { name: '142.251.40.238' }));

    const detail = (await screen.findByRole('heading', { name: '142.251.40.238' })).closest('section')!;
    expect(within(detail).getByText('calendar.google.com')).toBeTruthy();
    expect(within(detail).getByText('clients4.google.com')).toBeTruthy();
    expect(within(detail).getByText('chrome.exe')).toBeTruthy();
  });

  it('says plainly when no name was ever recorded for it', async () => {
    show(
      makeFakeApi({
        destinationHistory: vi.fn(async () => ({
          state: 'ok' as const,
          data: destinationHistoryView({ names: [] }),
        })),
      }),
    );
    await userEvent.click(await screen.findByRole('button', { name: '142.251.40.238' }));
    expect(await screen.findByText(/no name was recorded/i)).toBeTruthy();
  });

  it('can close the detail again', async () => {
    show();
    await userEvent.click(await screen.findByRole('button', { name: '142.251.40.238' }));
    await screen.findByLabelText('Traffic to this address over time');

    await userEvent.click(screen.getByRole('button', { name: 'Close' }));

    expect(screen.queryByLabelText('Traffic to this address over time')).toBeNull();
  });
});
