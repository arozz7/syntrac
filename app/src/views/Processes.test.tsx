// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { makeFakeApi } from '../test/fakeApi';
import { Processes } from './Processes';

afterEach(cleanup);

function show() {
  const api = makeFakeApi();
  render(
    <ApiProvider api={api}>
      <Processes />
    </ApiProvider>,
  );
  return api;
}

describe('Processes', () => {
  it('lists processes with their state spelled out', async () => {
    show();
    expect(await screen.findByText('worker.exe')).toBeTruthy();
    const row = (name: string) => screen.getByText(name).closest('tr')?.textContent ?? '';
    expect(row('worker.exe')).toContain('Running');
    expect(row('gone.exe')).toContain('Exited');
    expect(row('gone.exe')).not.toContain('approx');
    expect(row('lost.exe')).toContain('Exited (approx.)');
  });

  it('can limit the list to running processes', async () => {
    const api = show();
    await screen.findByText('worker.exe');
    expect(api.processes).toHaveBeenLastCalledWith(expect.objectContaining({ runningOnly: false }));

    await userEvent.click(screen.getByRole('checkbox', { name: 'Running only' }));
    await waitFor(() => expect(api.processes).toHaveBeenLastCalledWith(expect.objectContaining({ runningOnly: true })));
  });

  it('searches by name after a short pause rather than on every keystroke', async () => {
    const api = show();
    await screen.findByText('worker.exe');
    const before = api.processes.mock.calls.length;

    await userEvent.type(screen.getByRole('searchbox', { name: 'Search processes' }), 'chrome');

    await waitFor(() => expect(api.processes).toHaveBeenLastCalledWith(expect.objectContaining({ search: 'chrome' })), {
      timeout: 2000,
    });
    const searchCalls = api.processes.mock.calls.slice(before).filter(([params]) => params.search);
    expect(searchCalls.length).toBeLessThanOrEqual(2);
  });

  it('says how many processes are shown', async () => {
    show();
    expect(await screen.findByText(/3 processes/)).toBeTruthy();
  });
});
