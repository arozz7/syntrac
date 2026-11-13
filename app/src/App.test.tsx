// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { ApiProvider } from './api/ApiContext';
import { health, makeFakeApi } from './test/fakeApi';

vi.mock('./components/Chart', () => ({ Chart: () => <div data-testid="chart" /> }));

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <App />
    </ApiProvider>,
  );
  return api;
}

describe('App', () => {
  it('opens on the overview with the collector status visible at all times', async () => {
    show();
    expect(await screen.findByText('chrome.exe')).toBeTruthy();
    expect(screen.getByRole('status').getAttribute('data-state')).toBe('running');
    expect(screen.getByRole('heading', { level: 1 }).textContent).toContain('Overview');
  });

  it('navigates between the views and marks the current one', async () => {
    show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: 'Processes' }));
    expect(await screen.findByText('worker.exe')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Processes' }).getAttribute('aria-current')).toBe('page');

    await userEvent.click(screen.getByRole('button', { name: 'Connections' }));
    expect(await screen.findByText('browser.exe')).toBeTruthy();
  });

  it('opens the history view from the sidebar', async () => {
    const api = show();
    await screen.findByText('chrome.exe');

    await userEvent.click(screen.getByRole('button', { name: 'History' }));

    expect(await screen.findByText('CPU average')).toBeTruthy();
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('History');
    expect(api.systemHistory).toHaveBeenCalled();
  });

  it('keeps showing an offline collector in the banner while data from the past is still visible', async () => {
    show(
      makeFakeApi({
        health: vi.fn(async () => health({ state: 'offline', message: 'The collector is not running.', pid: null })),
      }),
    );
    expect(await screen.findByText('chrome.exe')).toBeTruthy();
    expect(screen.getByRole('status').getAttribute('data-state')).toBe('offline');
  });
});
