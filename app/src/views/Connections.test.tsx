// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { makeFakeApi } from '../test/fakeApi';
import { Connections } from './Connections';

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <Connections />
    </ApiProvider>,
  );
  return api;
}

describe('Connections', () => {
  it('shows who talked to what, with protocol and human sizes', async () => {
    show();
    expect(await screen.findByText('browser.exe')).toBeTruthy();
    const row = screen.getByText('browser.exe').closest('tr')?.textContent ?? '';
    expect(row).toContain('203.0.113.9:443');
    expect(row.toLowerCase()).toContain('tcp');
    expect(row).toContain('4.0 MB');
  });

  it('labels traffic from a process that could not be identified by its pid', async () => {
    show();
    expect(await screen.findByText('pid 99')).toBeTruthy();
    expect(screen.getByText('pid 99').closest('tr')?.textContent?.toLowerCase()).toContain('udp');
  });

  it('asks for a longer period when asked', async () => {
    const api = show();
    await screen.findByText('browser.exe');
    await userEvent.click(screen.getByRole('button', { name: '6 hours' }));
    await waitFor(() => expect(api.connections).toHaveBeenLastCalledWith(21600, expect.anything()));
  });

  it('shows the gate message when the database is missing', async () => {
    show(makeFakeApi({ connections: vi.fn(async () => ({ state: 'no_database' as const, message: 'missing' })) }));
    expect(await screen.findByRole('heading', { name: 'No data yet' })).toBeTruthy();
  });

  it('says so when nothing was seen in the period', async () => {
    show(makeFakeApi({ connections: vi.fn(async () => ({ state: 'ok' as const, data: [] })) }));
    expect(await screen.findByText(/No connections in this period/i)).toBeTruthy();
  });
});
