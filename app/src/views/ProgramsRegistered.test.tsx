// @vitest-environment jsdom
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { RegisteredView } from '../api/types';
import { identityDetail, makeFakeApi, programHistoryView } from '../test/fakeApi';
import { registeredDate } from '../identityLabels';
import { ProgramsHistory } from './ProgramsHistory';

vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

const NOW = new Date(2026, 10, 12, 9, 5, 7).getTime();

function registered(overrides: Partial<RegisteredView> = {}): RegisteredView {
  return {
    name: 'Acme',
    publisher: 'Acme Software, Inc.',
    version: '9.0.2.775',
    installed: '20260504',
    folder: 'c:\\program files (x86)\\acme',
    folderDerived: false,
    current: true,
    firstSeenMs: NOW - 5 * 86_400_000,
    lastSeenMs: NOW - 60_000,
    ...overrides,
  };
}

async function openPanel(entries: RegisteredView[]) {
  const api = makeFakeApi({
    programHistory: vi.fn(async () => ({
      state: 'ok' as const,
      data: programHistoryView({ identity: identityDetail({ registered: entries }) }),
    })),
  });
  render(
    <ApiProvider api={api}>
      <ProgramsHistory />
    </ApiProvider>,
  );
  await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
  return await screen.findByRole('region', { name: 'Identity' });
}

describe('registeredDate', () => {
  it('writes a registry date as a day, and leaves anything else as it is', () => {
    expect(registeredDate('20260504')).toBe('2026-05-04');
    expect(registeredDate('2026-05-04')).toBe('2026-05-04');
    expect(registeredDate('last spring')).toBe('last spring');
  });
});

describe('where a program came from, according to the installer records', () => {
  it('says what an installer registered for the folder, as a clue and not as proof', async () => {
    const panel = await openPanel([registered()]);

    const fact = within(panel).getByText('Registered by an installer as').closest('div')!;
    expect(within(fact).getByText('Acme')).toBeTruthy();
    expect(within(fact).getByText(/Acme Software, Inc./)).toBeTruthy();
    expect(within(fact).getByText(/9\.0\.2\.775/)).toBeTruthy();
    expect(within(fact).getByText(/installed 2026-05-04/)).toBeTruthy();
    expect(within(panel).getByText(/any program that can write the registry can claim a name/i)).toBeTruthy();
  });

  it('says plainly when nothing is registered for the folder, without saying the program is suspect', async () => {
    const panel = await openPanel([]);

    expect(within(panel).getByText(/no installer record names this folder/i)).toBeTruthy();
    expect(within(panel).queryByText(/suspicious|not legitimate/i)).toBeNull();
  });

  it('marks a record whose product has since been removed, and a folder that was only guessed', async () => {
    const panel = await openPanel([
      registered({ name: 'Old Tool', current: false }),
      registered({ name: 'Guessed Tool', folderDerived: true }),
    ]);

    expect(within(panel).getByText(/no longer installed/i)).toBeTruthy();
    expect(within(panel).getByText(/folder worked out from the uninstall command/i)).toBeTruthy();
  });

  it('shows several products registered for the same folder instead of choosing one', async () => {
    const panel = await openPanel([registered({ name: 'Product A' }), registered({ name: 'Product B' })]);

    expect(within(panel).getByText('Product A')).toBeTruthy();
    expect(within(panel).getByText('Product B')).toBeTruthy();
  });
});
