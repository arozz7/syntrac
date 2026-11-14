// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { KnownProductView, LookalikeView } from '../api/types';
import { knownProgramsView, makeFakeApi, programHistoryView, standing } from '../test/fakeApi';
import { KnownPrograms } from './KnownPrograms';

vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

const ACME: KnownProductView = {
  id: 1,
  label: 'Acme',
  publisher: 'acme software, inc.',
  folder: 'c:\\program files (x86)\\acme',
  productName: 'Acme Backup',
  note: 'my cloud backup',
  createdMs: new Date(2026, 10, 13, 9, 5).getTime(),
  programs: ['acmeserv.exe', 'acmetransmit64.exe'],
};

const FAKE: LookalikeView = {
  name: 'acmeserv.exe',
  path: 'C:\\Users\\a\\AppData\\Local\\Temp\\acmeserv.exe',
  standing: standing({
    state: 'lookalike',
    id: 1,
    label: 'Acme',
    expectedPublisher: 'acme software, inc.',
    expectedFolder: 'c:\\program files (x86)\\acme',
    reasons: ['it has the file name acmeserv.exe, like a program of Acme, but is not validly signed'],
  }),
  signature: 'unsigned',
  publisher: null,
  location: 'temporary',
};

function show(view = knownProgramsView({ products: [ACME], lookalikes: [FAKE] }), overrides = {}) {
  const api = makeFakeApi({
    knownPrograms: vi.fn(async () => ({ state: 'ok' as const, data: view })),
    ...overrides,
  });
  render(
    <ApiProvider api={api}>
      <KnownPrograms />
    </ApiProvider>,
  );
  return api;
}

describe('KnownPrograms', () => {
  it('lists each known product with its publisher, folder, programs, note and date', async () => {
    show();
    const row = (await screen.findByRole('button', { name: 'Remove Acme' })).closest('tr')!;
    expect(within(row).getByText('Acme')).toBeTruthy();
    expect(within(row).getByText('acme software, inc.')).toBeTruthy();
    expect(within(row).getByText('c:\\program files (x86)\\acme')).toBeTruthy();
    expect(within(row).getByText('acmeserv.exe, acmetransmit64.exe')).toBeTruthy();
    expect(within(row).getByText('my cloud backup')).toBeTruthy();
    expect(within(row).getByText('2026-11-13 09:05')).toBeTruthy();
  });

  it('explains what to do when the list is empty', async () => {
    show(knownProgramsView());
    expect(await screen.findByText(/no known programs yet/i)).toBeTruthy();
    expect(screen.getByText(/mark as known/i)).toBeTruthy();
    expect(screen.getByText(/nothing looks like a known program/i)).toBeTruthy();
  });

  it('asks before removing, then removes and refreshes', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'Remove Acme' }));
    expect(api.unmarkKnown).not.toHaveBeenCalled();
    expect(screen.getByText(/remove acme from the known programs\?/i)).toBeTruthy();
    const callsBefore = (api.knownPrograms as ReturnType<typeof vi.fn>).mock.calls.length;

    await userEvent.click(screen.getByRole('button', { name: 'Yes, remove' }));

    await waitFor(() => expect(api.unmarkKnown).toHaveBeenCalledWith(1));
    await waitFor(() =>
      expect((api.knownPrograms as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThan(callsBefore),
    );
  });

  it('keeps the product when the removal is called off', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'Remove Acme' }));

    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));

    expect(api.unmarkKnown).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Remove Acme' })).toBeTruthy();
  });

  it("shows the service's own words when a removal is refused", async () => {
    const refuse = vi.fn(async () => ({ ok: false, id: null, message: 'The collector is not running.' }));
    show(undefined, { unmarkKnown: refuse });
    await userEvent.click(await screen.findByRole('button', { name: 'Remove Acme' }));

    await userEvent.click(screen.getByRole('button', { name: 'Yes, remove' }));

    expect((await screen.findByRole('alert')).textContent).toMatch(/collector is not running/);
  });

  it('lists the programs that look like a known one, with why', async () => {
    show();
    const row = (await screen.findByRole('button', { name: 'acmeserv.exe' })).closest('tr')!;
    expect(within(row).getByText('Acme')).toBeTruthy();
    expect(within(row).getByText(/file name acmeserv\.exe/)).toBeTruthy();
    expect(within(row).getByText('Unsigned')).toBeTruthy();
    expect(within(row).getByText('runs from a temporary folder')).toBeTruthy();
  });

  it("opens a lookalike's detail", async () => {
    const api = show(undefined, {
      programHistory: vi.fn(async () => ({
        state: 'ok' as const,
        data: programHistoryView({ name: 'acmeserv.exe', path: FAKE.path }),
      })),
    });

    await userEvent.click(await screen.findByRole('button', { name: 'acmeserv.exe' }));

    await waitFor(() => expect(api.programHistory).toHaveBeenLastCalledWith(7 * 24 * 3600, FAKE.path));
    expect(await screen.findByRole('region', { name: 'Identity' })).toBeTruthy();
  });

  it('shows the gate message when there is no database', async () => {
    const api = makeFakeApi({
      knownPrograms: vi.fn(async () => ({ state: 'no_database' as const, message: 'There is no database.' })),
    });
    render(
      <ApiProvider api={api}>
        <KnownPrograms />
      </ApiProvider>,
    );
    expect(await screen.findByRole('heading', { name: 'No data yet' })).toBeTruthy();
  });
});
