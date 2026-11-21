// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { ApiProvider } from './api/ApiContext';
import { findingsView, health, makeFakeApi } from './test/fakeApi';

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

  it('shows how many findings are open next to Findings in the sidebar, and nothing when there are none', async () => {
    const api = makeFakeApi();
    render(
      <ApiProvider api={api}>
        <App />
      </ApiProvider>,
    );
    expect(await screen.findByRole('button', { name: /Findings.*2 open/i })).toBeTruthy();
    cleanup();

    const none = makeFakeApi({
      findings: vi.fn(async () => ({
        state: 'ok' as const,
        data: findingsView({ counts: { open: 0, notExpected: 0, snoozed: 0, expected: 0, resolved: 0 } }),
      })),
    });
    render(
      <ApiProvider api={none}>
        <App />
      </ApiProvider>,
    );
    expect(await screen.findByRole('button', { name: 'Findings' })).toBeTruthy();
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

describe('App: the notifications switch', () => {
  it('says what the program says: on by default, off when the program has them off', async () => {
    show();
    expect(await screen.findByRole('button', { name: 'Toasts: on' })).toBeTruthy();
    cleanup();

    show(makeFakeApi({ notificationsOn: vi.fn(async () => false) }));
    expect(await screen.findByRole('button', { name: 'Toasts: off' })).toBeTruthy();
  });

  it('asks the program to switch them, and shows the new state at once', async () => {
    const api = show();
    await userEvent.click(await screen.findByRole('button', { name: 'Toasts: on' }));

    expect(api.setNotificationsOn).toHaveBeenCalledWith(false);
    expect(await screen.findByRole('button', { name: 'Toasts: off' })).toBeTruthy();

    await userEvent.click(screen.getByRole('button', { name: 'Toasts: off' }));
    expect(api.setNotificationsOn).toHaveBeenLastCalledWith(true);
  });

  it('no longer sends toasts itself: the program does, with or without a window', async () => {
    const api = show();
    await screen.findByText('chrome.exe');
    expect(api).not.toHaveProperty('notify');
  });
});
