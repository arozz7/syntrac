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
    expect(api.topPrograms).toHaveBeenLastCalledWith(7 * 24 * 3600, 'cpu', undefined, false);
    expect(screen.getByRole('button', { name: '7 days' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'CPU' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('ranks again when another metric is chosen', async () => {
    const api = show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: 'Network' }));

    await waitFor(() => expect(api.topPrograms).toHaveBeenLastCalledWith(7 * 24 * 3600, 'network', undefined, false));
    expect(screen.getByRole('button', { name: 'Network' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'CPU' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('ranks again when another range is chosen', async () => {
    const api = show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: '30 days' }));

    await waitFor(() => expect(api.topPrograms).toHaveBeenLastCalledWith(30 * 24 * 3600, 'cpu', undefined, false));
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
    expect(screen.getByText(/five busiest on the GPU/i)).toBeTruthy();
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

describe('ProgramsHistory: the GPU', () => {
  it('shows the GPU peak and the GPU memory peak of each program', async () => {
    show(
      makeFakeApi({
        topPrograms: vi.fn(async () => ({
          state: 'ok' as const,
          data: programsView({ programs: [programRow({ gpuPeakPercent: 87.5, gpuMemoryPeakBytes: 2 * 1024 ** 3 })] }),
        })),
      }),
    );
    const row = (await screen.findByText('chrome.exe')).closest('tr')!;
    expect(within(row).getByText('87.5%')).toBeTruthy();
    expect(within(row).getByText('2.0 GB')).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: /GPU peak/ })).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: /GPU memory peak/ })).toBeTruthy();
  });

  it('ranks by the GPU when GPU is chosen and says what the figure is a share of', async () => {
    const api = show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: 'GPU' }));

    await waitFor(() => expect(api.topPrograms).toHaveBeenLastCalledWith(7 * 24 * 3600, 'gpu', undefined, false));
    expect(await screen.findByText(/busiest GPU engine/i)).toBeTruthy();
  });

  it('explains an empty GPU ranking in words about the GPU', async () => {
    const api = makeFakeApi({
      topPrograms: vi.fn(async (_range: number, metric: string) => ({
        state: 'ok' as const,
        data: metric === 'gpu' ? programsView({ programs: [] }) : programsView(),
      })),
    });
    show(api);
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: 'GPU' }));

    expect(await screen.findByText(/no program used the GPU/i)).toBeTruthy();
  });

  it('shows the GPU facts and chart of a program that used the GPU, and none for one that did not', async () => {
    const hours = programHistoryView().hours.map((hour, index) =>
      index === 0 ? { ...hour, gpuAvgPercent: 30, gpuPeakPercent: 64, gpuMemoryPeakBytes: 3 * 1024 ** 3 } : hour,
    );
    const used = makeFakeApi({
      programHistory: vi.fn(async () => ({ state: 'ok' as const, data: programHistoryView({ hours }) })),
    });
    show(used);
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
    expect(await screen.findByLabelText('Program GPU over time')).toBeTruthy();
    expect(screen.getByText('64.0%')).toBeTruthy();
    expect(screen.getByText('3.0 GB')).toBeTruthy();
    cleanup();

    show();
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
    await screen.findByLabelText('Program CPU over time');
    expect(screen.queryByLabelText('Program GPU over time')).toBeNull();
  });
});
