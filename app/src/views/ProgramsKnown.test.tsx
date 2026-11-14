// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { WriteOutcome } from '../api/types';
import {
  identityBadge,
  identityDetail,
  makeFakeApi,
  programHistoryView,
  programRow,
  programsView,
  standing,
} from '../test/fakeApi';
import { ProgramsHistory } from './ProgramsHistory';

vi.mock('../components/Chart', () => ({
  Chart: ({ label }: { label: string }) => <div data-testid="chart" aria-label={label} />,
}));

afterEach(cleanup);

const KNOWN = standing({ state: 'known', id: 1, label: 'Acme' });
const LOOKALIKE = standing({
  state: 'lookalike',
  id: 1,
  label: 'Acme',
  expectedPublisher: 'acme software, inc.',
  expectedFolder: 'c:\\program files (x86)\\acme',
  reasons: ['it has the file name acmeserv.exe, like a program of Acme, but is not validly signed'],
});

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <ProgramsHistory />
    </ApiProvider>,
  );
  return api;
}

function apiWith(row: ReturnType<typeof programRow>, extra: Parameters<typeof makeFakeApi>[0] = {}) {
  return makeFakeApi({
    topPrograms: vi.fn(async () => ({ state: 'ok' as const, data: programsView({ programs: [row] }) })),
    ...extra,
  });
}

describe('standing in the ranking', () => {
  it('marks a program of a known product with its label', async () => {
    show(apiWith(programRow({ standing: KNOWN })));
    const row = (await screen.findByText('chrome.exe')).closest('tr')!;
    expect(within(row).getByText('Known: Acme')).toBeTruthy();
  });

  it('flags a lookalike loudly and says why in the tooltip', async () => {
    show(apiWith(programRow({ standing: LOOKALIKE })));
    const flag = await screen.findByText('Looks like Acme');
    expect(flag.className).toMatch(/alert/);
    expect(flag.getAttribute('title')).toMatch(/file name acmeserv\.exe/);
  });

  it('shows nothing extra for a program that is not on the list', async () => {
    show(apiWith(programRow()));
    await screen.findByText('chrome.exe');
    expect(screen.queryByText(/^Known:/)).toBeNull();
    expect(screen.queryByText(/^Looks like/)).toBeNull();
  });
});

describe('marking a program as known', () => {
  async function open(detail = identityDetail(), historyStanding = standing(), api = makeFakeApi()) {
    const withHistory = makeFakeApi({
      ...api,
      programHistory: vi.fn(async () => ({
        state: 'ok' as const,
        data: programHistoryView({ identity: detail, standing: historyStanding }),
      })),
    });
    show(withHistory);
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
    const panel = await screen.findByRole('region', { name: 'Identity' });
    return { api: withHistory, panel };
  }

  it('offers the form with the product name as the suggested label', async () => {
    const { panel } = await open();

    await userEvent.click(within(panel).getByRole('button', { name: 'Mark as known' }));

    expect((within(panel).getByLabelText('Label') as HTMLInputElement).value).toBe('Google Chrome');
    expect(within(panel).getByLabelText('Note (optional)')).toBeTruthy();
  });

  it('suggests the publisher, then the program name, when there is no product name', async () => {
    const { panel } = await open(identityDetail({ product: null, publisher: 'Google LLC' }));
    await userEvent.click(within(panel).getByRole('button', { name: 'Mark as known' }));
    expect((within(panel).getByLabelText('Label') as HTMLInputElement).value).toBe('Google LLC');
  });

  it('sends the path, the label and the note, then refreshes the screen', async () => {
    const { api, panel } = await open();
    await userEvent.click(within(panel).getByRole('button', { name: 'Mark as known' }));
    const label = within(panel).getByLabelText('Label');
    await userEvent.clear(label);
    await userEvent.type(label, 'My browser');
    await userEvent.type(within(panel).getByLabelText('Note (optional)'), 'daily use');
    const rankingCallsBefore = (api.topPrograms as ReturnType<typeof vi.fn>).mock.calls.length;

    await userEvent.click(within(panel).getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(api.markKnown).toHaveBeenCalledWith('C:\\apps\\chrome.exe', 'My browser', 'daily use'));
    await waitFor(() =>
      expect((api.topPrograms as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThan(rankingCallsBefore),
    );
    expect(within(panel).queryByLabelText('Label')).toBeNull();
  });

  it("shows the service's own words when it refuses, and keeps the form open", async () => {
    const refuse = vi.fn(async (): Promise<WriteOutcome> => ({
      ok: false,
      id: null,
      message: 'This file has been changed since it was signed.',
    }));
    const { panel } = await open(identityDetail(), standing(), makeFakeApi({ markKnown: refuse }));
    await userEvent.click(within(panel).getByRole('button', { name: 'Mark as known' }));

    await userEvent.click(within(panel).getByRole('button', { name: 'Save' }));

    expect((await within(panel).findByRole('alert')).textContent).toMatch(/changed since it was signed/);
    expect(within(panel).getByLabelText('Label')).toBeTruthy();
  });

  it('can be cancelled without sending anything', async () => {
    const { api, panel } = await open();
    await userEvent.click(within(panel).getByRole('button', { name: 'Mark as known' }));

    await userEvent.click(within(panel).getByRole('button', { name: 'Cancel' }));

    expect(within(panel).queryByLabelText('Label')).toBeNull();
    expect(api.markKnown).not.toHaveBeenCalled();
  });

  it('does not offer to mark a program whose file has not been checked, and says why', async () => {
    const { panel } = await open(identityDetail({ state: 'pending', signature: null }));
    expect(within(panel).queryByRole('button', { name: 'Mark as known' })).toBeNull();
    expect(within(panel).getByText(/can be marked as known once its file has been checked/i)).toBeTruthy();
  });

  it('does not offer to mark a file that changed since it was signed', async () => {
    const { panel } = await open(identityDetail({ signature: 'altered' }));
    expect(within(panel).queryByRole('button', { name: 'Mark as known' })).toBeNull();
  });

  it('says a known program is known, and does not offer to mark it again', async () => {
    const { panel } = await open(identityDetail(), KNOWN);
    expect(within(panel).getByText(/known program: Acme/i)).toBeTruthy();
    expect(within(panel).queryByRole('button', { name: 'Mark as known' })).toBeNull();
  });

  it('explains a lookalike in plain words and lists the reasons', async () => {
    const { panel } = await open(identityBadgeDetailUnsigned(), LOOKALIKE);
    const warning = within(panel).getByRole('note');
    expect(warning.textContent).toMatch(/looks like Acme/i);
    expect(warning.textContent).toMatch(/acme software, inc./);
    expect(warning.textContent).toMatch(/file name acmeserv\.exe/);
  });
});

function identityBadgeDetailUnsigned() {
  return identityDetail({ ...identityBadge({ signature: 'unsigned', publisher: null }), signer: null });
}
