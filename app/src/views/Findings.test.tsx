// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { expectationRow, findingRow, findingsView, makeFakeApi } from '../test/fakeApi';
import { Findings } from './Findings';

afterEach(cleanup);

function show(api = makeFakeApi()) {
  render(
    <ApiProvider api={api}>
      <Findings />
    </ApiProvider>,
  );
  return api;
}

const ok = (data: ReturnType<typeof findingsView>) => ({ state: 'ok' as const, data });
const card = (title: string) => screen.getByRole('heading', { name: title }).closest('article')!;

describe('Findings: what was found', () => {
  it('opens on the findings that still need a look', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');
    expect(api.findings).toHaveBeenLastCalledWith('open');
    expect(screen.getByRole('button', { name: /Open \(2\)/ }).getAttribute('aria-pressed')).toBe('true');
  });

  it('shows each finding with its tier as a word and a shape, why it was raised and the evidence it used', async () => {
    show();
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');
    expect(within(first).getByText(/Suspicious/)).toBeTruthy();
    expect(within(first).getByText(/it is not signed/)).toBeTruthy();
    expect(within(first).getByText('Signature: not signed')).toBeTruthy();
    expect(within(first).getByText(/found 3 times/)).toBeTruthy();
    expect(within(card('New program other.exe')).getByText(/Unusual/)).toBeTruthy();
  });

  it('never calls a finding a verdict: it says it is evidence to look at', async () => {
    show();
    await screen.findByText('New program tool.exe');
    expect(screen.getByText(/not a verdict/i)).toBeTruthy();
    expect(screen.queryByText(/malicious|malware|virus/i)).toBeNull();
  });

  it('asks for the status that was chosen, with the counts on the buttons', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');

    await userEvent.click(screen.getByRole('button', { name: /Not expected \(1\)/ }));
    await waitFor(() => expect(api.findings).toHaveBeenLastCalledWith('not_expected'));
    await userEvent.click(screen.getByRole('button', { name: /^All/ }));
    await waitFor(() => expect(api.findings).toHaveBeenLastCalledWith(undefined));
  });

  it('says plainly when there is nothing to look at', async () => {
    show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [] }))) }));
    expect(await screen.findByText(/nothing here/i)).toBeTruthy();
  });
});

describe('Findings: is detection on', () => {
  it('says detection has not run yet and how "new" will be counted when it first does', async () => {
    show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ detection: null, findings: [] }))) }));
    expect(await screen.findByText(/has not run yet/i)).toBeTruthy();
    expect(screen.getByText(/already here/i)).toBeTruthy();
  });

  it('says since when detection has been on and when it last ran', async () => {
    show();
    expect(await screen.findByText(/on since/i)).toBeTruthy();
    expect(screen.getByText(/last ran/i)).toBeTruthy();
  });

  it('shows what went wrong in the last run', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(findingsView({ detection: { switchedOnMs: 1, lastRunMs: 2, lastError: 'fails: no data' } })),
        ),
      }),
    );
    expect((await screen.findByRole('alert')).textContent).toMatch(/fails: no data/);
  });
});

describe('Findings: teaching SYNTRAC what is expected', () => {
  it('"Expected this time" sends the decision and reads the list again', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');
    const before = api.findings.mock.calls.length;

    await userEvent.click(within(card('New program tool.exe')).getByRole('button', { name: 'Expected this time' }));

    expect(api.decideFinding).toHaveBeenCalledWith(1, 'expected_this_time', undefined);
    await waitFor(() => expect(api.findings.mock.calls.length).toBeGreaterThan(before));
  });

  it('"Always expected" asks for an optional note first, then sends it', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');

    await userEvent.click(within(first).getByRole('button', { name: 'Always expected' }));
    expect(api.decideFinding).not.toHaveBeenCalled();
    expect(within(first).getByText(/only this kind of finding is silenced/i)).toBeTruthy();
    await userEvent.type(within(first).getByRole('textbox', { name: /note/i }), 'my backup tool');
    await userEvent.click(within(first).getByRole('button', { name: 'Save as always expected' }));

    expect(api.decideFinding).toHaveBeenCalledWith(1, 'always_expected', { note: 'my backup tool' });
  });

  it('"Always expected" can cover everything in the program\'s folder, naming that folder first', async () => {
    const withFolder = findingRow({ folderScope: 'c:\\users\\someone\\.cargo' });
    const api = show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [withFolder] }))) }));
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');

    await userEvent.click(within(first).getByRole('button', { name: 'Always expected' }));
    expect(within(first).getByRole('radio', { name: /only this program/i })).toHaveProperty('checked', true);
    await userEvent.click(within(first).getByRole('radio', { name: /everything below c:\\users\\someone\\.cargo/i }));
    await userEvent.click(within(first).getByRole('button', { name: 'Save as always expected' }));

    expect(api.decideFinding).toHaveBeenCalledWith(1, 'always_expected', { scope: 'this_folder' });
  });

  it('offers no folder choice when the finding has no folder to offer', async () => {
    show();
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');

    await userEvent.click(within(first).getByRole('button', { name: 'Always expected' }));

    expect(within(first).queryByRole('radio')).toBeNull();
  });

  it('"Always expected" can be cancelled without sending anything', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');

    await userEvent.click(within(first).getByRole('button', { name: 'Always expected' }));
    await userEvent.click(within(first).getByRole('button', { name: 'Cancel' }));

    expect(api.decideFinding).not.toHaveBeenCalled();
    expect(within(first).queryByRole('textbox')).toBeNull();
  });

  it('"Not expected" keeps it visible as a concern', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');

    await userEvent.click(within(card('New program tool.exe')).getByRole('button', { name: 'Not expected' }));

    expect(api.decideFinding).toHaveBeenCalledWith(1, 'not_expected', undefined);
  });

  it('a snooze names how many days', async () => {
    const api = show();
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');

    await userEvent.selectOptions(within(first).getByRole('combobox', { name: /snooze for/i }), '7');
    await userEvent.click(within(first).getByRole('button', { name: 'Snooze' }));

    expect(api.decideFinding).toHaveBeenCalledWith(1, 'snooze', { days: 7 });
  });

  it('shows why a decision was refused and leaves the finding as it was', async () => {
    const api = makeFakeApi({
      decideFinding: vi.fn(async () => ({ ok: false, id: null, message: 'The collector is not running.' })),
    });
    show(api);
    await screen.findByText('New program tool.exe');
    const before = api.findings.mock.calls.length;

    await userEvent.click(within(card('New program tool.exe')).getByRole('button', { name: 'Not expected' }));

    expect((await within(card('New program tool.exe')).findByRole('alert')).textContent).toMatch(/not running/);
    expect(api.findings.mock.calls.length).toBe(before);
  });

  it('a decided finding says what was decided and offers to undo it', async () => {
    const decided = findingRow({
      id: 9,
      status: 'expected',
      title: 'New program done.exe',
      decidedAtMs: 1_700_000_000_000,
    });
    const api = show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [decided] }))) }));
    await screen.findByText('New program done.exe');
    const done = card('New program done.exe');

    expect(within(done).getByText(/you said this is expected/i)).toBeTruthy();
    expect(within(done).queryByRole('button', { name: 'Expected this time' })).toBeNull();
    await userEvent.click(within(done).getByRole('button', { name: 'Reopen' }));
    expect(api.decideFinding).toHaveBeenCalledWith(9, 'reopen', undefined);
  });

  it('a snoozed finding says until when', async () => {
    const snoozed = findingRow({
      id: 8,
      status: 'snoozed',
      title: 'New program zzz.exe',
      snoozedUntilMs: 1_800_000_000_000,
    });
    show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [snoozed] }))) }));
    await screen.findByText('New program zzz.exe');
    expect(within(card('New program zzz.exe')).getByText(/snoozed until/i)).toBeTruthy();
  });
});

describe('Findings: what you told SYNTRAC is expected', () => {
  it('lists each expectation with its note and who it covers, exactly', async () => {
    show();
    const row = (await screen.findByText(/acmeserv\.exe/)).closest('tr')!;
    expect(within(row).getByText('New program')).toBeTruthy();
    expect(within(row).getByText('my backup')).toBeTruthy();
  });

  it('removes one only after asking, and reads the lists again', async () => {
    const api = show();
    await screen.findByText(/acmeserv\.exe/);
    const before = api.findings.mock.calls.length;

    await userEvent.click(screen.getByRole('button', { name: /^Remove/ }));
    expect(api.removeExpectation).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: 'Yes, remove' }));

    expect(api.removeExpectation).toHaveBeenCalledWith(5);
    await waitFor(() => expect(api.findings.mock.calls.length).toBeGreaterThan(before));
  });

  it('can keep an expectation instead of removing it', async () => {
    const api = show();
    await screen.findByText(/acmeserv\.exe/);

    await userEvent.click(screen.getByRole('button', { name: /^Remove/ }));
    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));

    expect(api.removeExpectation).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Yes, remove' })).toBeNull();
  });

  it('says so when nothing has been marked as expected', async () => {
    show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ expectations: [] }))) }));
    expect(await screen.findByText(/you have not marked anything as expected/i)).toBeTruthy();
  });

  it('keeps the expectation row stable when the list holds other kinds too', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              expectations: [expectationRow(), expectationRow({ id: 6, kind: 'odd_kind_name', subject: 'x' })],
            }),
          ),
        ),
      }),
    );
    expect(await screen.findByText('odd kind name')).toBeTruthy();
  });
});
