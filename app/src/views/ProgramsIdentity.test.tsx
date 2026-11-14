// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import {
  identityBadge,
  identityDetail,
  makeFakeApi,
  programHistoryView,
  programRow,
  programsView,
} from '../test/fakeApi';
import { ProgramsHistory } from './ProgramsHistory';

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

function withPrograms(...programs: ReturnType<typeof programRow>[]) {
  return makeFakeApi({
    topPrograms: vi.fn(async () => ({ state: 'ok' as const, data: programsView({ programs }) })),
  });
}

const ACME = programRow({
  name: 'Acme Backup',
  path: null,
  cpuAvgPercent: 0.75,
  identity: identityBadge({ publisher: 'ACME SOFTWARE, INC.', product: 'Acme Backup' }),
  members: [
    {
      name: 'acmeserv.exe',
      path: 'C:\\Program Files (x86)\\Acme\\acmeserv.exe',
      identity: identityBadge({ publisher: 'ACME SOFTWARE, INC.' }),
    },
    {
      name: 'acmetransmit64.exe',
      path: 'C:\\Program Files (x86)\\Acme\\x64\\acmetransmit64.exe',
      identity: identityBadge({ publisher: 'ACME SOFTWARE, INC.' }),
    },
  ],
});

describe('identity in the ranking', () => {
  it('shows who made each program and whether it is signed', async () => {
    show();
    const chrome = (await screen.findByText('chrome.exe')).closest('tr')!;
    expect(within(chrome).getByText('Signed')).toBeTruthy();
    expect(within(chrome).getByText('Google LLC')).toBeTruthy();
  });

  it('says honestly when a program has not been checked yet or could not be read', async () => {
    const pending = identityBadge({ state: 'pending', signature: null, publisher: null });
    const unreadable = identityBadge({ state: 'unreadable', signature: null, publisher: null });
    show(
      withPrograms(
        programRow({ name: 'new.exe', path: 'C:\\a\\new.exe', identity: pending }),
        programRow({ name: 'gone.exe', path: 'C:\\a\\gone.exe', identity: unreadable }),
      ),
    );
    expect(await screen.findByText('Not yet checked')).toBeTruthy();
    expect(screen.getByText('Could not read the file')).toBeTruthy();
  });

  it('notes a program that runs from a temporary or user folder, and stays quiet about Program Files', async () => {
    const temporary = identityBadge({ signature: 'unsigned', publisher: null, location: 'temporary' });
    show(
      withPrograms(
        programRow({ name: 'a.exe', path: 'C:\\t\\a.exe', identity: temporary }),
        programRow({ name: 'b.exe', path: 'C:\\p\\b.exe', identity: identityBadge({ location: 'program_files' }) }),
      ),
    );
    const temp = (await screen.findByText('a.exe')).closest('tr')!;
    expect(within(temp).getByText('Unsigned')).toBeTruthy();
    expect(within(temp).getByText('runs from a temporary folder')).toBeTruthy();
    const installed = screen.getByText('b.exe').closest('tr')!;
    expect(within(installed).queryByText(/runs from/)).toBeNull();
  });

  it('says whether the publisher is the signer or only a name written in the file', async () => {
    show(
      withPrograms(
        programRow({ name: 'a.exe', path: 'C:\\a\\a.exe', identity: identityBadge({ publisher: 'Signer Inc.' }) }),
        programRow({
          name: 'b.exe',
          path: 'C:\\b\\b.exe',
          identity: identityBadge({ signature: 'unsigned', publisher: 'Company Ltd' }),
        }),
      ),
    );
    expect((await screen.findByText('Signer Inc.')).getAttribute('title')).toMatch(/signed by/i);
    const claimed = screen.getByText('Company Ltd').getAttribute('title') ?? '';
    expect(claimed).toMatch(/version information/i);
    expect(claimed).toMatch(/anyone can write/i);
  });

  it('gives a changed-since-signed file the loudest label', async () => {
    show(withPrograms(programRow({ identity: identityBadge({ signature: 'altered' }) })));
    const label = await screen.findByText('Changed since signed');
    expect(label.className).toMatch(/alert/);
  });
});

describe('group by product', () => {
  it('is off at first, and asks for the grouped ranking when switched on', async () => {
    const api = show();
    await screen.findByText('chrome.exe');
    const toggle = screen.getByRole('button', { name: 'Group by product' });
    expect(toggle.getAttribute('aria-pressed')).toBe('false');

    await userEvent.click(toggle);

    await waitFor(() => expect(api.topPrograms).toHaveBeenLastCalledWith(7 * 24 * 3600, 'cpu', undefined, true));
    expect(screen.getByRole('button', { name: 'Group by product' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('shows a product as one row with its program count, and lists the programs when opened', async () => {
    show(withPrograms(ACME));
    const row = (await screen.findByText('2 programs')).closest('tr')!;
    expect(screen.queryByText('acmeserv.exe')).toBeNull();

    await userEvent.click(within(row).getByRole('button', { name: /Acme Backup/ }));

    expect(screen.getByText('acmeserv.exe')).toBeTruthy();
    expect(screen.getByText('acmetransmit64.exe')).toBeTruthy();
    expect(
      within(row)
        .getByRole('button', { name: /Acme Backup/ })
        .getAttribute('aria-expanded'),
    ).toBe('true');
  });

  it('opens the detail of one program of a product', async () => {
    const api = show(withPrograms(ACME));
    await userEvent.click(await screen.findByRole('button', { name: /Acme Backup/ }));

    await userEvent.click(screen.getByRole('button', { name: 'acmeserv.exe' }));

    await waitFor(() =>
      expect(api.programHistory).toHaveBeenLastCalledWith(
        7 * 24 * 3600,
        'C:\\Program Files (x86)\\Acme\\acmeserv.exe',
      ),
    );
    expect(await screen.findByRole('heading', { name: 'acmeserv.exe' })).toBeTruthy();
  });
});

describe('the identity panel of one program', () => {
  async function open(detail = identityDetail()) {
    const api = makeFakeApi({
      programHistory: vi.fn(async () => ({
        state: 'ok' as const,
        data: programHistoryView({ identity: detail }),
      })),
    });
    show(api);
    await userEvent.click(await screen.findByRole('button', { name: /chrome\.exe/ }));
    return await screen.findByRole('region', { name: 'Identity' });
  }

  it('lists what was measured in plain words', async () => {
    const panel = await open();
    expect(within(panel).getByText('Signed')).toBeTruthy();
    expect(within(panel).getAllByText('Google LLC').length).toBeGreaterThan(0);
    expect(within(panel).getByText('AB12CD34')).toBeTruthy();
    expect(within(panel).getByText('126.0.1')).toBeTruthy();
    expect(within(panel).getByText('ab'.repeat(32))).toBeTruthy();
    expect(within(panel).getByText('1.0 MB')).toBeTruthy();
    expect(within(panel).getByText('Program Files')).toBeTruthy();
    expect(within(panel).getByText(/no download mark/i)).toBeTruthy();
  });

  it('says a file was downloaded from the Internet when it carries that mark', async () => {
    const panel = await open(identityDetail({ downloaded: true }));
    expect(within(panel).getByText(/downloaded from the internet/i)).toBeTruthy();
  });

  it('says that nothing is known yet for a program that has not been checked', async () => {
    const panel = await open(
      identityDetail({
        state: 'pending',
        signature: null,
        publisher: null,
        product: null,
        signer: null,
        signatureKind: null,
        thumbprint: null,
        company: null,
        description: null,
        originalFilename: null,
        fileVersion: null,
        sha256: null,
        sizeBytes: null,
        modifiedMs: null,
        inspectedAtMs: null,
        lastAttemptMs: null,
      }),
    );
    expect(within(panel).getByText('Not yet checked')).toBeTruthy();
    expect(within(panel).queryByText('Signed by')).toBeNull();
  });

  it('reports a failed look and keeps what was learnt earlier', async () => {
    const panel = await open(identityDetail({ failure: { kind: 'not_found', message: 'no such file', attempts: 2 } }));
    expect(within(panel).getByText(/last attempt to read this file failed/i)).toBeTruthy();
    expect(within(panel).getByText(/no such file/)).toBeTruthy();
    expect(within(panel).getByText('Signed')).toBeTruthy();
  });

  it('treats a different built-in file name as information, not as a warning', async () => {
    const panel = await open(identityDetail({ originalFilename: 'chrome_proxy.exe' }));
    const note = within(panel).getByText(/built with a different name/i);
    expect(note.textContent).toMatch(/common/i);
    expect(note.className).not.toMatch(/alert|caution/);
  });

  it('reminds the reader what these signals cannot prove', async () => {
    const panel = await open();
    expect(within(panel).getByText(/none of this proves a program is safe/i)).toBeTruthy();
  });
});
