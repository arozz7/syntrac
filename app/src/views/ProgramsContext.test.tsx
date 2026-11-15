// @vitest-environment jsdom
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import {
  identityBadge,
  identityDetail,
  makeFakeApi,
  programHistoryView,
  programRow,
  programsView,
} from '../test/fakeApi';
import { ProgramsHistory } from './ProgramsHistory';

vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

const DAY = 86_400_000;
const NOW = new Date(2026, 10, 12, 9, 5, 7).getTime();

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <ProgramsHistory />
    </ApiProvider>,
  );
  return api;
}

function withPrograms(...programs: ReturnType<typeof programRow>[]) {
  return makeFakeApi({
    topPrograms: vi.fn(async () => ({ state: 'ok' as const, data: programsView({ programs }) })),
  });
}

async function openPanel(detail = identityDetail()) {
  const api = makeFakeApi({
    programHistory: vi.fn(async () => ({
      state: 'ok' as const,
      data: programHistoryView({ identity: detail }),
    })),
  });
  show(api);
  await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
  return await screen.findByRole('region', { name: 'Identity' });
}

describe('how a program runs, in the ranking', () => {
  it('marks a program that has been seen hosting a Windows service, quietly', async () => {
    show(
      withPrograms(
        programRow({ name: 'acmeserv.exe', identity: identityBadge({ runsAs: ['system'], service: true }) }),
        programRow({ name: 'chrome.exe', path: 'C:\\apps\\chrome.exe' }),
      ),
    );
    const serv = (await screen.findByText('acmeserv.exe')).closest('tr')!;
    const badge = within(serv).getByText('Service');
    expect(badge.className).toContain('badge--quiet');
    const chrome = screen.getByText('chrome.exe').closest('tr')!;
    expect(within(chrome).queryByText('Service')).toBeNull();
  });
});

describe('how a program runs, in the identity panel', () => {
  it('lists the accounts it has been seen running as, with their kind and when', async () => {
    const panel = await openPanel(
      identityDetail({
        runsAs: ['system', 'user'],
        accounts: [
          { name: 'NT AUTHORITY\\SYSTEM', kind: 'system', firstSeenMs: NOW - 3 * DAY, lastSeenMs: NOW - 1000 },
          { name: 'PC\\alice', kind: 'user', firstSeenMs: NOW - 2 * DAY, lastSeenMs: NOW - DAY },
        ],
      }),
    );

    const runsAs = within(panel).getByText('Runs as').closest('div')!;
    expect(within(runsAs).getByText(/NT AUTHORITY\\SYSTEM/)).toBeTruthy();
    expect(within(runsAs).getByText(/PC\\alice/)).toBeTruthy();
    expect(within(runsAs).getByText(/SYSTEM account/)).toBeTruthy();
    expect(within(runsAs).getByText(/user account/i)).toBeTruthy();
  });

  it('lists the services it has been seen hosting', async () => {
    const panel = await openPanel(
      identityDetail({
        service: true,
        services: [
          { name: 'acmeserv', displayName: 'Acme Service', firstSeenMs: NOW - DAY, lastSeenMs: NOW - 1000 },
          { name: 'Dnscache', displayName: null, firstSeenMs: NOW - DAY, lastSeenMs: NOW - 2000 },
        ],
      }),
    );

    const services = within(panel).getByText('Services').closest('div')!;
    expect(within(services).getByText(/acmeserv/)).toBeTruthy();
    expect(within(services).getByText(/Acme Service/)).toBeTruthy();
    expect(within(services).getByText(/Dnscache/)).toBeTruthy();
  });

  it('says plainly that it was not observed yet, rather than claiming it has no account', async () => {
    const panel = await openPanel(identityDetail({ runsAs: [], accounts: [], services: [], service: false }));

    expect(within(panel).getByText(/not observed running yet/i)).toBeTruthy();
    expect(within(panel).queryByText('Services')).toBeNull();
  });

  it('points out a program with a high-privilege account that runs from a temporary folder, as a thing to look at', async () => {
    const panel = await openPanel(
      identityDetail({
        location: 'temporary',
        runsAs: ['system'],
        accounts: [{ name: 'NT AUTHORITY\\SYSTEM', kind: 'system', firstSeenMs: NOW - DAY, lastSeenMs: NOW }],
      }),
    );

    expect(within(panel).getByText(/worth a look/i)).toBeTruthy();
  });

  it('stays quiet about SYSTEM from Program Files: that is how system software runs', async () => {
    const panel = await openPanel(
      identityDetail({
        location: 'program_files',
        runsAs: ['system'],
        accounts: [{ name: 'NT AUTHORITY\\SYSTEM', kind: 'system', firstSeenMs: NOW - DAY, lastSeenMs: NOW }],
      }),
    );

    expect(within(panel).queryByText(/worth a look/i)).toBeNull();
  });

  it('reminds the reader that an account is evidence of how a program was started, not of whether it is safe', async () => {
    const panel = await openPanel();

    expect(within(panel).getByText(/how a program was started/i)).toBeTruthy();
  });
});
