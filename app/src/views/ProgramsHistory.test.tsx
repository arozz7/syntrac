// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { makeFakeApi, programRow, programsView, programHistoryView } from '../test/fakeApi';
import { ProgramsHistory } from './ProgramsHistory';

// jsdom has no canvas; the charts are exercised through their pure option builders.
vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <ProgramsHistory />
    </ApiProvider>,
  );
  return api;
}

describe('ProgramsHistory', () => {
  it('lists the ranked programs with their figures in human units', async () => {
    show();
    const chrome = (await screen.findByText('chrome.exe')).closest('tr')!;
    expect(within(chrome).getByText('10.0%')).toBeTruthy(); // CPU average
    expect(within(chrome).getByText('20.0%')).toBeTruthy(); // CPU peak
    expect(within(chrome).getByText('1.0 GB')).toBeTruthy(); // memory peak
    expect(within(chrome).getByText('1 of 2 h')).toBeTruthy();
    expect(screen.getByText('code.exe')).toBeTruthy();
  });

  it('opens on the last 7 days ranked by CPU', async () => {
    const api = show();
    await screen.findByText('chrome.exe');
    expect(api.topPrograms).toHaveBeenLastCalledWith(7 * 24 * 3600, 'cpu');
    expect(screen.getByRole('button', { name: '7 days' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'CPU' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('ranks again when another metric is chosen', async () => {
    const api = show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: 'Network' }));

    await waitFor(() => expect(api.topPrograms).toHaveBeenLastCalledWith(7 * 24 * 3600, 'network'));
    expect(screen.getByRole('button', { name: 'Network' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'CPU' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('ranks again when another range is chosen', async () => {
    const api = show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: '30 days' }));

    await waitFor(() => expect(api.topPrograms).toHaveBeenLastCalledWith(30 * 24 * 3600, 'cpu'));
  });

  it('shows one program hour by hour when its row is chosen', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));

    expect(await screen.findByLabelText('Program CPU over time')).toBeTruthy();
    expect(screen.getByLabelText('Program memory over time')).toBeTruthy();
    expect(api.programHistory).toHaveBeenLastCalledWith(7 * 24 * 3600, 'C:\\apps\\chrome.exe');
    expect(screen.getByRole('heading', { name: 'chrome.exe' })).toBeTruthy();
  });

  it('can close the detail again', async () => {
    show();
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
    await screen.findByLabelText('Program CPU over time');

    await userEvent.click(screen.getByRole('button', { name: 'Close' }));

    expect(screen.queryByLabelText('Program CPU over time')).toBeNull();
  });

  it('does not offer a drill-down for the unknown-program bucket', async () => {
    show(
      makeFakeApi({
        topPrograms: vi.fn(async () => ({
          state: 'ok' as const,
          data: programsView({ programs: [programRow({ name: 'Unknown program', path: null })] }),
        })),
      }),
    );
    expect(await screen.findByText('Unknown program')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Unknown program/ })).toBeNull();
  });

  it('says what is left out of a ranking and how the average is worked out', async () => {
    show();
    await screen.findByText('chrome.exe');
    expect(screen.getByText(/25 busiest processes/i)).toBeTruthy();
    expect(screen.getByText(/counts those times as zero/i)).toBeTruthy();
    expect(screen.getByText(/excludes traffic to this machine itself/i)).toBeTruthy();
  });

  it('says where the history begins when the range reaches back further than it', async () => {
    show(
      makeFakeApi({
        topPrograms: vi.fn(async () => ({
          state: 'ok' as const,
          data: programsView({ fromMs: 0, historyStartsMs: 129_600_000 }),
        })),
      }),
    );
    expect(await screen.findByText(/history begins 1970-01-02/i)).toBeTruthy();
  });

  it('explains an empty ranking instead of drawing an empty table', async () => {
    show(
      makeFakeApi({ topPrograms: vi.fn(async () => ({ state: 'ok' as const, data: programsView({ programs: [] }) })) }),
    );
    expect(await screen.findByText(/no programs to rank/i)).toBeTruthy();
    expect(screen.queryByRole('table')).toBeNull();
  });

  it('shows the gate message instead of numbers when there is no database', async () => {
    show(
      makeFakeApi({
        topPrograms: vi.fn(async () => ({ state: 'no_database' as const, message: 'There is no database.' })),
      }),
    );
    expect(await screen.findByRole('heading', { name: 'No data yet' })).toBeTruthy();
  });

  it('shows the detail facts: first and last seen and the starts in the range', async () => {
    show(
      makeFakeApi({
        programHistory: vi.fn(async () => ({
          state: 'ok' as const,
          data: programHistoryView({ firstSeenMs: new Date(2026, 10, 12, 9, 5).getTime() }),
        })),
      }),
    );
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
    expect(await screen.findByText('2026-11-12 09:05')).toBeTruthy();
  });
});
