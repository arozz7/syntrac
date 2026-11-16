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
  ownerDataState,
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
    expect(within(row).getByRole('button', { name: 'code.exe' })).toBeTruthy();
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
    expect(screen.getByRole('dialog', { name: '142.251.40.238' })).toBeTruthy();
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

    const detail = await screen.findByRole('dialog', { name: '142.251.40.238' });
    expect(await within(detail).findByText('calendar.google.com')).toBeTruthy();
    expect(within(detail).getByText('clients4.google.com')).toBeTruthy();
    expect(within(detail).getByRole('button', { name: 'chrome.exe' })).toBeTruthy();
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
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('closes with Escape and gives focus back to the address that was clicked', async () => {
    show();
    const address = await screen.findByRole('button', { name: '142.251.40.238' });
    await userEvent.click(address);
    await screen.findByLabelText('Traffic to this address over time');

    await userEvent.keyboard('{Escape}');

    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(address);
  });
});

describe('NetworkHistory: who owns the network', () => {
  const OWNER = { asn: 15169, org: 'GOOGLE', country: 'US' };

  it('shows the owner of the network range under the name, as a clue and not as the service', async () => {
    show(
      makeFakeApi({
        topDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: destinationsView({ destinations: [destinationRow({ owner: OWNER })] }),
        })),
      }),
    );
    const row = (await screen.findByRole('button', { name: '142.251.40.238' })).closest('tr')!;

    expect(within(row).getByText('Network AS15169 GOOGLE, registered in US')).toBeTruthy();
    expect(screen.getByText(/not necessarily the service/i)).toBeTruthy();
  });

  it('says nothing about an owner when none is known for an address', async () => {
    show();
    const row = (await screen.findByRole('button', { name: '142.251.40.238' })).closest('tr')!;

    expect(within(row).queryByText(/^Network AS/)).toBeNull();
  });

  it('shows the owner in the new destinations and in the detail', async () => {
    show(
      makeFakeApi({
        newDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: newDestinationsView({ destinations: [newDestinationRow({ owner: OWNER })] }),
        })),
        destinationHistory: vi.fn(async () => ({
          state: 'ok' as const,
          data: destinationHistoryView({ owner: OWNER }),
        })),
      }),
    );
    const row = (await screen.findByRole('button', { name: '198.51.100.7' })).closest('tr')!;
    expect(within(row).getByText('Network AS15169 GOOGLE, registered in US')).toBeTruthy();

    await userEvent.click(within(row).getByRole('button', { name: '198.51.100.7' }));

    const dialog = await screen.findByRole('dialog', { name: '198.51.100.7' });
    expect(await within(dialog).findByText('Network AS15169 GOOGLE, registered in US')).toBeTruthy();
  });

  it('reads the screen again when new owner data has been downloaded', async () => {
    let call = 0;
    const api = makeFakeApi({
      ownerDataStatus: vi.fn(async () => ({
        ok: true,
        message: null,
        state: ownerDataState({
          dataset: { source: 'iptoasn.com', downloadedMs: call++ < 1 ? 1 : 2, rows: 5 },
        }),
      })),
    });
    show(api);
    await screen.findByRole('button', { name: '142.251.40.238' });
    const before = api.topDestinations.mock.calls.length;

    // The bar looks again when asked to refresh: the stamp has moved, so the table must be read again too.
    await userEvent.click(await screen.findByRole('button', { name: 'Refresh owner data' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Download' }));

    await waitFor(() => expect(api.topDestinations.mock.calls.length).toBeGreaterThan(before));
  });
});

describe('NetworkHistory: seeing the programs', () => {
  const MANY = destinationRow({
    programs: 6,
    topPrograms: [
      { name: 'chrome.exe', path: 'C:\\apps\\chrome.exe' },
      { name: 'Teams.exe', path: 'C:\\apps\\Teams.exe' },
      { name: 'Unknown program', path: null },
    ],
    morePrograms: 3,
  });

  function showMany() {
    return show(
      makeFakeApi({
        topDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: destinationsView({ destinations: [MANY] }),
        })),
      }),
    );
  }

  it('names the biggest programs in the row and counts the rest', async () => {
    showMany();
    const row = (await screen.findByRole('button', { name: '142.251.40.238' })).closest('tr')!;

    expect(within(row).getByRole('button', { name: 'chrome.exe' })).toBeTruthy();
    expect(within(row).getByRole('button', { name: 'Teams.exe' })).toBeTruthy();
    expect(within(row).getByText('Unknown program')).toBeTruthy();
    expect(within(row).getByRole('button', { name: '+3 more' })).toBeTruthy();
  });

  it('does not make the unidentified bucket a link, and says nothing about more when there is none', async () => {
    show();
    const row = (await screen.findByRole('button', { name: '203.0.113.50' })).closest('tr')!;

    expect(within(row).getByText('Unknown program').closest('button')).toBeNull();
    expect(within(row).queryByText(/more/)).toBeNull();
  });

  it('opens that program’s history in a dialog when its name is clicked in the row', async () => {
    const api = showMany();
    const row = (await screen.findByRole('button', { name: '142.251.40.238' })).closest('tr')!;

    await userEvent.click(within(row).getByRole('button', { name: 'Teams.exe' }));

    const dialog = await screen.findByRole('dialog', { name: 'Teams.exe' });
    await waitFor(() => expect(api.programHistory).toHaveBeenLastCalledWith(WEEK, 'C:\\apps\\Teams.exe'));
    expect(within(dialog).getByRole('button', { name: /back to 142\.251\.40\.238/i })).toBeTruthy();
  });

  it('opens the destination from “+N more”', async () => {
    showMany();
    const row = (await screen.findByRole('button', { name: '142.251.40.238' })).closest('tr')!;

    await userEvent.click(within(row).getByRole('button', { name: '+3 more' }));

    expect(await screen.findByRole('dialog', { name: '142.251.40.238' })).toBeTruthy();
  });

  it('opens a program from inside the destination and goes back to the destination', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: '142.251.40.238' }));
    const destination = await screen.findByRole('dialog', { name: '142.251.40.238' });

    await userEvent.click(await within(destination).findByRole('button', { name: 'chrome.exe' }));

    const program = await screen.findByRole('dialog', { name: 'chrome.exe' });
    await waitFor(() => expect(api.programHistory).toHaveBeenLastCalledWith(WEEK, 'C:\\apps\\chrome.exe'));

    await userEvent.click(within(program).getByRole('button', { name: /back to 142\.251\.40\.238/i }));

    expect(await screen.findByRole('dialog', { name: '142.251.40.238' })).toBeTruthy();
    expect(screen.queryByRole('dialog', { name: 'chrome.exe' })).toBeNull();
  });

  it('opens the program that was first to talk to a new destination', async () => {
    const api = show();
    const row = (await screen.findByRole('button', { name: '198.51.100.7' })).closest('tr')!;

    await userEvent.click(within(row).getByRole('button', { name: 'code.exe' }));

    await screen.findByRole('dialog', { name: 'code.exe' });
    await waitFor(() => expect(api.programHistory).toHaveBeenLastCalledWith(WEEK, 'C:\\apps\\code.exe'));
  });
});
