// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { gpuProcess, makeFakeApi, overview } from '../test/fakeApi';
import { Overview } from './Overview';

// jsdom has no canvas; the chart itself is exercised through its pure option builder.
vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <Overview />
    </ApiProvider>,
  );
  return api;
}

describe('Overview', () => {
  it('names who is using the CPU and shows their share', async () => {
    show();
    expect(await screen.findByText('chrome.exe')).toBeTruthy();
    expect(screen.getByText('MsMpEng.exe')).toBeTruthy();
    expect(screen.getByText('3.2%')).toBeTruthy();
  });

  it('shows the CPU that no process explains as its own labelled row, with the reason', async () => {
    show();
    expect(await screen.findByText('Not attributed to any process')).toBeTruthy();
    expect(screen.getByText('4.9%')).toBeTruthy();
    expect(screen.getByText(/interrupt and driver time/i)).toBeTruthy();
    expect(screen.getByText('Everything else')).toBeTruthy();
  });

  it('states the system average the rows add up to', async () => {
    show();
    expect(await screen.findByText(/system average/i)).toBeTruthy();
    expect(screen.getByText(/12\.0%/)).toBeTruthy();
  });

  it('does not repeat the pid for a process that is already named by its pid', async () => {
    const data = overview();
    data.cpu.processes = [{ ...data.cpu.processes[0]!, name: 'pid 555', pid: 555, instance: null }];
    show(makeFakeApi({ overview: vi.fn(async () => ({ state: 'ok' as const, data })) }));
    expect(await screen.findAllByText(/pid 555/)).toHaveLength(1);
  });

  it('lists the busiest network destinations with human sizes', async () => {
    show();
    expect(await screen.findByText('57.144.180.192:443')).toBeTruthy();
    expect(screen.getByText('22.3 MB')).toBeTruthy();
  });

  it('draws the system chart', async () => {
    show();
    expect(await screen.findByTestId('chart')).toBeTruthy();
  });

  it('asks again for a different range when a range button is pressed', async () => {
    const api = show();
    await screen.findByText('chrome.exe');
    expect(api.overview).toHaveBeenLastCalledWith(900);

    await userEvent.click(screen.getByRole('button', { name: '1 hour' }));
    await waitFor(() => expect(api.overview).toHaveBeenLastCalledWith(3600));
    expect(screen.getByRole('button', { name: '1 hour' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: '15 min' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('shows the gate message instead of numbers when there is no database', async () => {
    show(
      makeFakeApi({
        overview: vi.fn(async () => ({ state: 'no_database' as const, message: 'There is no database.' })),
      }),
    );
    expect(await screen.findByRole('heading', { name: 'No data yet' })).toBeTruthy();
    expect(screen.queryByText('chrome.exe')).toBeNull();
  });

  it('handles a quiet system with nothing to list', async () => {
    const empty = overview({
      system: [],
      destinations: [],
      cpu: { windowSamples: 0, systemAvgPercent: 0, othersAvgPercent: 0, unaccountedAvgPercent: 0, processes: [] },
    });
    show(makeFakeApi({ overview: vi.fn(async () => ({ state: 'ok' as const, data: empty })) }));
    expect(await screen.findByText(/No samples in this period/i)).toBeTruthy();
  });
});

describe('Overview: the GPU', () => {
  const gpuOverview = () =>
    overview({
      system: overview().system.map((point, index) => ({ ...point, gpuPercent: index === 1 ? 64 : 20 })),
      gpu: {
        readingSamples: 170,
        systemAvgPercent: 31,
        systemPeakPercent: 97,
        othersPeakPercent: 2.5,
        processes: [gpuProcess('toolx.exe', 42, 7, 3 * 1024 ** 3), gpuProcess('chrome.exe', 6, 1, 512 * 1024 ** 2)],
      },
    });
  const showGpu = (data = gpuOverview()) =>
    show(makeFakeApi({ overview: vi.fn(async () => ({ state: 'ok' as const, data })) }));

  it('names who is using the GPU with each program share, peak and GPU memory', async () => {
    showGpu();
    expect(await screen.findByRole('heading', { name: 'Who is using the GPU' })).toBeTruthy();
    expect(screen.getByText('toolx.exe')).toBeTruthy();
    expect(screen.getByText('42.0%')).toBeTruthy();
    expect(screen.getByText('peak 84.0%')).toBeTruthy();
    expect(screen.getByText(/3\.0 GB GPU memory/)).toBeTruthy();
  });

  it('says what the figures are a share of, the busiest engine of the machine, and the largest unlisted share', async () => {
    showGpu();
    expect(await screen.findByText(/busiest GPU engine/i)).toBeTruthy();
    expect(screen.getByText(/system average 31\.0%/i)).toBeTruthy();
    expect(screen.getByText(/peak 97\.0%/i)).toBeTruthy();
    expect(screen.getByText(/programs not listed used at most 2\.5%/i)).toBeTruthy();
  });

  it('shows the GPU now next to CPU and memory when the latest point has a reading', async () => {
    showGpu();
    expect(await screen.findByText('GPU now')).toBeTruthy();
    expect(screen.getByText('64.0%')).toBeTruthy();
  });

  it('says the GPU was not recorded, rather than showing an idle GPU, when no sample had a reading', async () => {
    show();
    expect(await screen.findByRole('heading', { name: 'Who is using the GPU' })).toBeTruthy();
    expect(screen.getByText(/gpu use is not recorded for this period/i)).toBeTruthy();
    expect(screen.queryByText('GPU now')).toBeNull();
  });

  it('says so when there were readings but no program used the GPU', async () => {
    const data = gpuOverview();
    data.gpu = { ...data.gpu, processes: [], othersPeakPercent: 0 };
    showGpu(data);
    expect(await screen.findByText(/no program used the gpu/i)).toBeTruthy();
  });
});
