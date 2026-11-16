// @vitest-environment jsdom
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { LauncherView } from '../api/types';
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

const NOW = new Date(2026, 10, 12, 9, 5, 7).getTime();
const DAY = 86_400_000;

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <ProgramsHistory />
    </ApiProvider>,
  );
  return api;
}

function launcher(overrides: Partial<LauncherView> = {}): LauncherView {
  return {
    kind: 'service_auto',
    location: 'acmeserv',
    detail: 'automatic',
    arguments: '-noconsole',
    firstSeenMs: NOW - 5 * DAY,
    lastSeenMs: NOW - 60_000,
    current: true,
    ...overrides,
  };
}

async function openPanel(launchers: LauncherView[]) {
  const api = makeFakeApi({
    programHistory: vi.fn(async () => ({
      state: 'ok' as const,
      data: programHistoryView({ identity: identityDetail({ launchers }) }),
    })),
  });
  show(api);
  await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
  return await screen.findByRole('region', { name: 'Identity' });
}

describe('how a program gets started, in the ranking', () => {
  it('marks a program that starts with Windows, quietly, and says nothing about one that does not', async () => {
    show(
      makeFakeApi({
        topPrograms: vi.fn(async () => ({
          state: 'ok' as const,
          data: programsView({
            programs: [
              programRow({ name: 'acmeserv.exe', identity: identityBadge({ startsAtBoot: true }) }),
              programRow({ name: 'chrome.exe', path: 'C:\\apps\\chrome.exe' }),
            ],
          }),
        })),
      }),
    );

    const serv = (await screen.findByText('acmeserv.exe')).closest('tr')!;
    const badge = within(serv).getByText('Starts at boot');
    expect(badge.className).toContain('badge--quiet');
    expect(within(screen.getByText('chrome.exe').closest('tr')!).queryByText('Starts at boot')).toBeNull();
  });
});

describe('how a program gets started, in the identity panel', () => {
  it('lists each entry with what kind it is, where it was found and since when', async () => {
    const panel = await openPanel([
      launcher(),
      launcher({
        kind: 'task',
        location: '\\Vendor\\Updater',
        detail: 'boot, logon',
        arguments: '/check',
        firstSeenMs: NOW - 2 * DAY,
      }),
      launcher({ kind: 'run_key', location: 'HKLM\\Run\\Sync', detail: '', arguments: '' }),
      launcher({ kind: 'startup_folder', location: 'Startup (all users)\\Sync.lnk', detail: '', arguments: '' }),
    ]);

    const fact = within(panel).getByText('Starts with Windows via').closest('div')!;
    expect(within(fact).getByText(/Windows service \(automatic\)/)).toBeTruthy();
    expect(within(fact).getByText('acmeserv')).toBeTruthy();
    expect(within(fact).getByText(/Scheduled task/)).toBeTruthy();
    expect(within(fact).getByText('\\Vendor\\Updater')).toBeTruthy();
    expect(within(fact).getByText(/boot, logon/)).toBeTruthy();
    expect(within(fact).getByText(/Run key/)).toBeTruthy();
    expect(within(fact).getByText(/Startup folder/)).toBeTruthy();
    expect(within(fact).getByText(/-noconsole/)).toBeTruthy();
    expect(within(fact).getAllByText(/since/i).length).toBeGreaterThanOrEqual(4);
  });

  it('says plainly that nothing was found, and where it looked, never that the program does not autostart', async () => {
    const panel = await openPanel([]);

    expect(within(panel).getByText(/no start-up entry found/i)).toBeTruthy();
    expect(within(panel).getByText(/services, scheduled tasks, Run keys and Startup folders/i)).toBeTruthy();
    expect(within(panel).queryByText(/does not autostart|never starts/i)).toBeNull();
  });

  it('marks an entry that is no longer configured instead of dropping it', async () => {
    const panel = await openPanel([launcher({ kind: 'run_key', location: 'HKLM\\Run\\Old', current: false })]);

    expect(within(panel).getByText(/no longer configured/i)).toBeTruthy();
    expect(within(panel).getByText('HKLM\\Run\\Old')).toBeTruthy();
  });

  it('shows a service that only starts on demand as a quiet fact', async () => {
    const panel = await openPanel([launcher({ kind: 'service_other', detail: 'on demand' })]);

    expect(within(panel).getByText(/Windows service \(on demand\)/)).toBeTruthy();
  });

  it('says what it cannot see, so a missing entry is not read as a clean bill', async () => {
    const panel = await openPanel([launcher()]);

    expect(within(panel).getByText(/not read:/i)).toBeTruthy();
    expect(within(panel).getByText(/started by another program/i)).toBeTruthy();
  });
});
