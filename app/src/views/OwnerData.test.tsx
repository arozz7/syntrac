// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { OwnerDataOutcome, OwnerDataStateView } from '../api/types';
import { NOW, makeFakeApi, ownerDataState } from '../test/fakeApi';
import { OwnerDataBar } from './OwnerDataBar';

afterEach(cleanup);

const outcome = (state: OwnerDataStateView): OwnerDataOutcome => ({ ok: true, state, message: null });

function show(
  state: OwnerDataStateView | OwnerDataOutcome,
  options: { onDownloaded?: () => void; refresh?: OwnerDataOutcome } = {},
) {
  const status = 'ok' in state ? state : outcome(state);
  // Once the user has agreed, the service reports that it is downloading.
  let started = false;
  const api = makeFakeApi({
    ownerDataStatus: vi.fn(async () => (started ? outcome(ownerDataState({ refreshing: true })) : status)),
    refreshOwnerData: vi.fn(async () => {
      const answer = options.refresh ?? outcome(ownerDataState({ refreshing: true }));
      started = answer.ok;
      return answer;
    }),
  });
  render(
    <ApiProvider api={api}>
      <OwnerDataBar nowMs={NOW} activePollMs={20} onDownloaded={options.onDownloaded ?? (() => undefined)} />
    </ApiProvider>,
  );
  return api;
}

describe('OwnerDataBar', () => {
  it('says plainly that nothing has been downloaded and what that means', async () => {
    show(ownerDataState({ dataset: null }));

    expect(await screen.findByText(/owner data: not downloaded/i)).toBeTruthy();
    expect(screen.getByText(/only the names programs asked for/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Download owner data' })).toBeTruthy();
  });

  it('asks first, saying exactly what leaves the machine, and downloads nothing until agreed', async () => {
    const api = show(ownerDataState({ dataset: null }));

    await userEvent.click(await screen.findByRole('button', { name: 'Download owner data' }));

    const dialog = await screen.findByRole('dialog', { name: /download owner data/i });
    expect(within(dialog).getByText(/one public-domain file from iptoasn\.com/i)).toBeTruthy();
    expect(within(dialog).getByText(/sends no address of yours/i)).toBeTruthy();
    expect(within(dialog).getByText(/only when you press/i)).toBeTruthy();
    expect(api.refreshOwnerData).not.toHaveBeenCalled();
  });

  it('downloads nothing when cancelled or when Escape is pressed', async () => {
    const api = show(ownerDataState({ dataset: null }));
    await userEvent.click(await screen.findByRole('button', { name: 'Download owner data' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).toBeNull();

    await userEvent.click(screen.getByRole('button', { name: 'Download owner data' }));
    await screen.findByRole('dialog');
    await userEvent.keyboard('{Escape}');

    expect(screen.queryByRole('dialog')).toBeNull();
    expect(api.refreshOwnerData).not.toHaveBeenCalled();
  });

  it('asks the service to download once it is agreed, and shows that it is under way', async () => {
    const api = show(ownerDataState({ dataset: null }));
    await userEvent.click(await screen.findByRole('button', { name: 'Download owner data' }));

    await userEvent.click(await screen.findByRole('button', { name: 'Download' }));

    await waitFor(() => expect(api.refreshOwnerData).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(await screen.findByText(/downloading owner data/i)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Download owner data' })).toBeNull();
  });

  it('keeps asking while it downloads and tells the window when the new data has arrived', async () => {
    const downloaded = vi.fn();
    const downloading = outcome(ownerDataState({ dataset: null, refreshing: true }));
    const done = outcome(ownerDataState({ dataset: { source: 'iptoasn.com', downloadedMs: NOW, rows: 480_000 } }));
    let call = 0;
    const api = makeFakeApi({ ownerDataStatus: vi.fn(async () => (call++ < 2 ? downloading : done)) });
    render(
      <ApiProvider api={api}>
        <OwnerDataBar nowMs={NOW} activePollMs={20} onDownloaded={downloaded} />
      </ApiProvider>,
    );

    expect(await screen.findByText(/downloading owner data/i)).toBeTruthy();
    expect(await screen.findByText(/480,000 network ranges/i)).toBeTruthy();
    await waitFor(() => expect(downloaded).toHaveBeenCalledTimes(1));
  });

  it('does not report a download the first time it looks at data that was already there', async () => {
    const downloaded = vi.fn();
    show(ownerDataState(), { onDownloaded: downloaded });

    await screen.findByText(/downloaded/i);

    expect(downloaded).not.toHaveBeenCalled();
  });

  it('says where and when the data came from and offers a refresh', async () => {
    show(ownerDataState({ dataset: { source: 'iptoasn.com', downloadedMs: NOW - 3 * 86_400_000, rows: 480_000 } }));

    expect(await screen.findByText(/downloaded 2026-11-09 from iptoasn\.com/i)).toBeTruthy();
    expect(screen.getByText(/480,000 network ranges/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Refresh owner data' })).toBeTruthy();
    expect(screen.queryByText(/days old/i)).toBeNull();
  });

  it('says when the data is getting old', async () => {
    show(ownerDataState({ dataset: { source: 'iptoasn.com', downloadedMs: NOW - 45 * 86_400_000, rows: 10 } }));

    expect(await screen.findByText(/45 days old/i)).toBeTruthy();
  });

  it('reports a failed download in the service own words, and keeps the data it had', async () => {
    show(ownerDataState({ lastError: 'the download failed: the server answered 503' }));

    expect(await screen.findByText(/last attempt failed/i)).toBeTruthy();
    expect(screen.getByText(/the server answered 503/)).toBeTruthy();
    expect(screen.getByText(/480,000 network ranges/i)).toBeTruthy();
  });

  it('says so when downloading is switched off, and offers no button', async () => {
    show(ownerDataState({ dataset: null, allowed: false }));

    expect(await screen.findByText(/switched off/i)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /owner data/i })).toBeNull();
  });

  it('says why when the service cannot be reached instead of showing a stale state', async () => {
    show({ ok: false, state: null, message: 'The SYNTRAC collector is not running, so owner data cannot be checked.' });

    expect(await screen.findByText(/collector is not running/i)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /owner data/i })).toBeNull();
  });

  it('shows a refusal from the service when the download could not be started', async () => {
    show(ownerDataState({ dataset: null }), {
      refresh: { ok: false, state: null, message: 'Could not start the download: no threads' },
    });
    await userEvent.click(await screen.findByRole('button', { name: 'Download owner data' }));

    await userEvent.click(await screen.findByRole('button', { name: 'Download' }));

    expect(await screen.findByText(/could not start the download/i)).toBeTruthy();
  });
});
