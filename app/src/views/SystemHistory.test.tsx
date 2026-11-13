// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { SystemHistoryView } from '../api/types';
import { makeFakeApi, NOW, systemHistory, systemHour } from '../test/fakeApi';
import { SystemHistory } from './SystemHistory';

// jsdom has no canvas; the charts are exercised through their pure option builders.
vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <SystemHistory />
    </ApiProvider>,
  );
  return api;
}

const withHistory = (history: SystemHistoryView) =>
  makeFakeApi({ systemHistory: vi.fn(async () => ({ state: 'ok' as const, data: history })) });

describe('SystemHistory', () => {
  it('draws the load, network and disk charts', async () => {
    show();
    expect(await screen.findByLabelText('CPU and memory over time')).toBeTruthy();
    expect(screen.getByLabelText('Network traffic over time')).toBeTruthy();
    expect(screen.getByLabelText('Disk traffic over time')).toBeTruthy();
  });

  it('opens on the last 7 days and asks for that range', async () => {
    const api = show();
    await screen.findByLabelText('CPU and memory over time');
    expect(api.systemHistory).toHaveBeenLastCalledWith(7 * 24 * 3600);
    expect(screen.getByRole('button', { name: '7 days' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('offers a day, a week, a month, a quarter, a year and everything', () => {
    show();
    for (const label of ['24 hours', '7 days', '30 days', '90 days', '1 year', 'All']) {
      expect(screen.getByRole('button', { name: label })).toBeTruthy();
    }
  });

  it('asks again for another range when a range button is pressed', async () => {
    const api = show();
    await screen.findByLabelText('CPU and memory over time');

    await userEvent.click(screen.getByRole('button', { name: '30 days' }));

    await waitFor(() => expect(api.systemHistory).toHaveBeenLastCalledWith(30 * 24 * 3600));
    expect(screen.getByRole('button', { name: '30 days' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('summarises the range: weighted CPU average, peaks and the traffic moved', async () => {
    show(
      withHistory(
        systemHistory({
          hours: [
            systemHour(NOW - 3 * 3_600_000, {
              samples: 100,
              cpuAvgPercent: 10,
              cpuMaxPercent: 40,
              netReceivedBytes: 1024,
            }),
            systemHour(NOW - 2 * 3_600_000, {
              samples: 300,
              cpuAvgPercent: 30,
              cpuMaxPercent: 80,
              netReceivedBytes: 2048,
            }),
          ],
        }),
      ),
    );
    expect(await screen.findByText('25.0%')).toBeTruthy(); // (10*100 + 30*300) / 400
    expect(screen.getByText('80.0%')).toBeTruthy(); // the highest peak
    expect(screen.getByText('3.0 KB')).toBeTruthy(); // received in total
  });

  it('says where the history begins when the range reaches back further than it', async () => {
    show(withHistory(systemHistory({ fromMs: NOW - 90 * 86_400_000, historyStartsMs: NOW - 86_400_000 })));
    expect(await screen.findByText(/history begins 2026-11-11/i)).toBeTruthy();
  });

  it('does not claim a start of history when the range is inside it', async () => {
    show(withHistory(systemHistory({ fromMs: NOW - 86_400_000, historyStartsMs: NOW - 5 * 86_400_000 })));
    await screen.findByLabelText('CPU and memory over time');
    expect(screen.queryByText(/history begins/i)).toBeNull();
  });

  it('explains an empty history instead of drawing empty charts', async () => {
    show(withHistory(systemHistory({ hours: [], historyStartsMs: null })));
    expect(await screen.findByText(/no hourly summaries yet/i)).toBeTruthy();
    expect(screen.queryByTestId('chart')).toBeNull();
  });

  it('explains that the newest hour appears a few minutes after it ends', async () => {
    show();
    expect(await screen.findByText(/only completed hours/i)).toBeTruthy();
  });

  it('shows the gate message instead of numbers when there is no database', async () => {
    show(
      makeFakeApi({
        systemHistory: vi.fn(async () => ({ state: 'no_database' as const, message: 'There is no database.' })),
      }),
    );
    expect(await screen.findByRole('heading', { name: 'No data yet' })).toBeTruthy();
  });
});
