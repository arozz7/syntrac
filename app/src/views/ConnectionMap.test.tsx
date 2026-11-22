// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { ConnectionMapView } from '../api/types';
import { connectionMapView, makeFakeApi } from '../test/fakeApi';
import { ConnectionMap } from './ConnectionMap';

// jsdom has no canvas: the chart is replaced by buttons that click the first program and the first network, the way a click on a node does.
vi.mock('../components/Chart', () => ({
  Chart: ({ label, onSelect }: { label: string; onSelect?: (data: unknown) => void }) => (
    <div data-testid="chart" aria-label={label}>
      <button type="button" onClick={() => onSelect?.({ side: 'program', index: 1 })}>
        click second program node
      </button>
      <button type="button" onClick={() => onSelect?.({ side: 'network', index: 0 })}>
        click first network node
      </button>
      <button type="button" onClick={() => onSelect?.({ source: 'p:0', target: 'n:0', value: 5 })}>
        click a band
      </button>
    </div>
  ),
}));

afterEach(cleanup);

const WEEK = 7 * 24 * 3600;

function apiWith(view: ConnectionMapView) {
  return makeFakeApi({ connectionMap: vi.fn(async () => ({ state: 'ok' as const, data: view })) });
}

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <ConnectionMap />
    </ApiProvider>,
  );
  return api;
}

describe('ConnectionMap', () => {
  it('draws the chart and says in words what it shows', async () => {
    show();
    expect(await screen.findByTestId('chart')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Connection map' })).toBeTruthy();
  });

  it('lists every link in a table with the traffic in human units, for people who cannot use the chart', async () => {
    show();
    const rows = await screen.findAllByRole('row');
    const text = rows.map((row) => row.textContent ?? '');
    const chrome = text.find((row) => row.includes('chrome.exe') && row.includes('EXAMPLE-NET'))!;
    expect(chrome).toContain('4.8 MB');
    expect(text.some((row) => row.includes('code.exe') && row.includes('976.6 KB'))).toBe(true);
    expect(text.some((row) => row.includes('chrome.exe') && row.includes('No owner found'))).toBe(true);
  });

  it('reads a week by default and another range when asked', async () => {
    const api = show();
    await screen.findByTestId('chart');
    expect(api.connectionMap).toHaveBeenLastCalledWith(WEEK, expect.objectContaining({ allAddresses: false }));
    await userEvent.click(screen.getByRole('button', { name: '24 hours' }));
    await waitFor(() =>
      expect(api.connectionMap).toHaveBeenLastCalledWith(24 * 3600, expect.objectContaining({ allAddresses: false })),
    );
  });

  it('can include this network and multicast', async () => {
    const api = show();
    await screen.findByTestId('chart');
    await userEvent.click(screen.getByRole('button', { name: 'Include this network and multicast' }));
    await waitFor(() =>
      expect(api.connectionMap).toHaveBeenLastCalledWith(WEEK, expect.objectContaining({ allAddresses: true })),
    );
  });

  it('opens the details of a program from the table', async () => {
    const api = show();
    const row = (await screen.findAllByRole('row')).find((r) => r.textContent?.includes('code.exe'))!;
    await userEvent.click(within(row).getByRole('button', { name: 'code.exe' }));
    await waitFor(() => expect(api.programHistory).toHaveBeenCalledWith(WEEK, String.raw`C:\Apps\code.exe`));
  });

  it('opens the details of a program when its node in the chart is clicked', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'click second program node' }));
    await waitFor(() => expect(api.programHistory).toHaveBeenCalledWith(WEEK, String.raw`C:\Apps\code.exe`));
  });

  it('shows the owner and the biggest addresses of a network when its node is clicked, and opens an address', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'click first network node' }));
    const panel = screen.getByRole('region', { name: 'EXAMPLE-NET Example Networks, Inc.' });
    expect(within(panel).getByText(/AS64500/)).toBeTruthy();
    expect(within(panel).getByText('www.example.com')).toBeTruthy();
    await userEvent.click(within(panel).getByRole('button', { name: /203\.0\.113\.9/ }));
    await waitFor(() => expect(api.destinationHistory).toHaveBeenCalledWith(WEEK, '203.0.113.9'));
  });

  it('does nothing when a band between nodes is clicked', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'click a band' }));
    expect(api.programHistory).not.toHaveBeenCalled();
    expect(screen.queryByRole('region', { name: /EXAMPLE-NET/ })).toBeNull();
  });

  it('shows a folded-together node as text, since there is no one program to open', async () => {
    const view = connectionMapView();
    view.programs.push({
      kind: 'others',
      name: '3 other programs',
      path: null,
      programs: 3,
      bytesSent: 5,
      bytesReceived: 5,
    });
    view.links.push({ program: 2, network: 0, bytesSent: 5, bytesReceived: 5 });
    show(apiWith(view));
    const row = (await screen.findAllByRole('row')).find((r) => r.textContent?.includes('3 other programs'))!;
    expect(within(row).queryByRole('button', { name: '3 other programs' })).toBeNull();
  });

  it('says how many were left out of the chart', async () => {
    show(apiWith(connectionMapView({ totalPrograms: 40, totalNetworks: 2 })));
    expect(await screen.findByText(/2 busiest of 40 programs/)).toBeTruthy();
  });

  it('says so when no program talked to anything in the period', async () => {
    show(apiWith(connectionMapView({ programs: [], networks: [], links: [], totalPrograms: 0, totalNetworks: 0 })));
    expect(await screen.findByText(/No traffic to show/)).toBeTruthy();
    expect(screen.queryByTestId('chart')).toBeNull();
  });

  it('explains what an owner is and that a network is not a service', async () => {
    show();
    await screen.findByTestId('chart');
    expect(screen.getByText(/organisation that runs the range of addresses/)).toBeTruthy();
  });
});
