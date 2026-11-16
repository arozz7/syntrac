// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { makeFakeApi, programDestinationRow, programDestinationsView } from '../test/fakeApi';
import { ProgramDestinations } from './ProgramDestinations';

afterEach(cleanup);

const WEEK = 7 * 24 * 3600;
const PATH = 'C:\\apps\\chrome.exe';

function show(api = makeFakeApi(), onOpenDestination?: (addr: string) => void) {
  render(
    <ApiProvider api={api}>
      <ProgramDestinations path={PATH} rangeSecs={WEEK} onOpenDestination={onOpenDestination} />
    </ApiProvider>,
  );
  return api;
}

describe('ProgramDestinations', () => {
  it('asks for this program over the range', async () => {
    const api = show();
    await screen.findByText('142.251.40.238');
    expect(api.programDestinations).toHaveBeenLastCalledWith(WEEK, PATH, undefined, undefined);
  });

  it('lists each destination with its name, owner, traffic and first time this program reached it', async () => {
    show();
    const row = (await screen.findByText('142.251.40.238')).closest('tr')!;
    expect(within(row).getByText('calendar.google.com')).toBeTruthy();
    expect(within(row).getByText(/AS15169 GOOGLE/)).toBeTruthy();
    expect(within(row).getByText('5.0 MB')).toBeTruthy(); // received
    expect(within(row).getByText('2.0 KB')).toBeTruthy(); // sent
    expect(within(row).getByText('40')).toBeTruthy(); // flows
  });

  it('marks a destination this program reached for the first time in the range as new, and only that one', async () => {
    show();
    const fresh = (await screen.findByText('203.0.113.50')).closest('tr')!;
    expect(within(fresh).getByText('New')).toBeTruthy();
    const old = screen.getByText('142.251.40.238').closest('tr')!;
    expect(within(old).queryByText('New')).toBeNull();
  });

  it('says when no name or owner was found rather than leaving a gap', async () => {
    show();
    const row = (await screen.findByText('203.0.113.50')).closest('tr')!;
    expect(within(row).getByText('No name seen')).toBeTruthy();
    expect(within(row).getByText('No owner found')).toBeTruthy();
  });

  it('opens a destination when the page offers to, and shows plain text when it does not', async () => {
    const chosen = vi.fn();
    show(makeFakeApi(), chosen);
    await userEvent.click(await screen.findByRole('button', { name: '142.251.40.238' }));
    expect(chosen).toHaveBeenCalledWith('142.251.40.238');

    cleanup();
    show();
    await screen.findByText('142.251.40.238');
    expect(screen.queryByRole('button', { name: '142.251.40.238' })).toBeNull();
  });

  it('says how many more destinations there are than the list shows', async () => {
    show(
      makeFakeApi({
        programDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: programDestinationsView({ totalDestinations: 90 }),
        })),
      }),
    );
    expect(await screen.findByText(/biggest 2 of 90 destinations/i)).toBeTruthy();
  });

  it('says plainly when the program talked to no address on the internet in the range', async () => {
    show(
      makeFakeApi({
        programDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: programDestinationsView({ destinations: [], totalDestinations: 0 }),
        })),
      }),
    );
    expect(await screen.findByText(/no destinations on the internet/i)).toBeTruthy();
  });

  it('sorts by a column heading', async () => {
    const small = programDestinationRow({ addr: '192.0.2.5', bytesReceived: 5, bytesSent: 5, flows: 1 });
    show(
      makeFakeApi({
        programDestinations: vi.fn(async () => ({
          state: 'ok' as const,
          data: programDestinationsView({ destinations: [programDestinationRow(), small], totalDestinations: 2 }),
        })),
      }),
    );
    await screen.findByText('142.251.40.238');
    const addresses = () =>
      screen
        .getAllByRole('row')
        .slice(1)
        .map((row) => within(row).getAllByRole('cell')[0]?.textContent ?? '');
    expect(addresses()[0]).toMatch(/142\.251/);

    await userEvent.click(screen.getByRole('button', { name: 'Flows' })); // biggest first: unchanged
    await waitFor(() => expect(addresses()[0]).toMatch(/142\.251/));
    await userEvent.click(screen.getByRole('button', { name: 'Flows' })); // smallest first
    expect(addresses()[0]).toMatch(/192\.0\.2\.5/);
  });
});
