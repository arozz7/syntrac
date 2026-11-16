// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { makeFakeApi, ownersView } from '../test/fakeApi';
import { TopOwners } from './TopOwners';

afterEach(cleanup);

const WEEK = 7 * 24 * 3600;

function show(api = makeFakeApi(), onSelect: (addr: string) => void = () => undefined) {
  render(
    <ApiProvider api={api}>
      <TopOwners rangeSecs={WEEK} allAddresses={false} refreshKey={0} onSelect={onSelect} />
    </ApiProvider>,
  );
  return api;
}

const ownerNames = () =>
  screen
    .getAllByRole('row')
    .slice(1)
    .map((row) => within(row).getAllByRole('cell')[0]?.textContent ?? '');

describe('TopOwners', () => {
  it('asks for the range and whether to include this network', async () => {
    const api = show();
    await screen.findByText(/GOOGLE/);
    expect(api.topOwners).toHaveBeenLastCalledWith(WEEK, undefined, false);
  });

  it('lists each owner with its network number, the country it is registered in, and its totals', async () => {
    show();
    const row = (await screen.findByRole('button', { name: /GOOGLE/ })).closest('tr')!;
    expect(within(row).getByText(/AS15169/)).toBeTruthy();
    expect(within(row).getByText(/registered in US/)).toBeTruthy();
    expect(within(row).getByText('8.0 MB')).toBeTruthy(); // received
    expect(within(row).getByText('4.0 KB')).toBeTruthy(); // sent
    expect(within(row).getByText('7')).toBeTruthy(); // destinations
  });

  it('calls the group with no owner "No owner found" instead of guessing', async () => {
    show();
    const row = (await screen.findByRole('button', { name: /No owner found/ })).closest('tr')!;
    expect(within(row).getByText('2')).toBeTruthy();
  });

  it("shows a row's biggest destinations when it is opened, each one opening that address, and says how many more there are", async () => {
    const chosen = vi.fn();
    show(makeFakeApi(), chosen);
    const owner = await screen.findByRole('button', { name: /GOOGLE/ });
    expect(owner.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('button', { name: '142.251.40.238' })).toBeNull();

    await userEvent.click(owner);

    expect(owner.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('calendar.google.com')).toBeTruthy();
    expect(screen.getByText('+5 more destinations')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '142.251.40.238' }));
    expect(chosen).toHaveBeenCalledWith('142.251.40.238');

    await userEvent.click(owner);
    expect(screen.queryByRole('button', { name: '142.251.40.238' })).toBeNull();
  });

  it('says plainly that owners need the downloaded data when none is on this machine', async () => {
    const api = makeFakeApi({
      topOwners: vi.fn(async () => ({
        state: 'ok' as const,
        data: ownersView({ ownerData: null, owners: [ownersView().owners[2]!] }),
      })),
    });
    show(api);
    expect(await screen.findByText(/no owner data on this machine yet/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: /No owner found/ })).toBeTruthy();
  });

  it('sorts by a column heading', async () => {
    show();
    await screen.findByRole('button', { name: /GOOGLE/ });
    expect(ownerNames()[0]).toMatch(/GOOGLE/);

    await userEvent.click(screen.getByRole('button', { name: 'Destinations' })); // biggest first
    await waitFor(() => expect(ownerNames()[0]).toMatch(/GOOGLE/));
    await userEvent.click(screen.getByRole('button', { name: 'Destinations' })); // smallest first
    expect(ownerNames()[0]).toMatch(/AMAZON/);
  });

  it('says so when nothing was recorded', async () => {
    show(makeFakeApi({ topOwners: vi.fn(async () => ({ state: 'ok' as const, data: ownersView({ owners: [] }) })) }));
    expect(await screen.findByText(/no destinations to group/i)).toBeTruthy();
  });
});
