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

describe('Findings: the last day and what is new since you looked', () => {
  it('counts the new addresses and networks of the last day, as a digest and not an alert', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              digest: { windowHours: 24, newDestinations: 14, newOwners: 3, ownerData: true, newInBuildFolders: 0 },
            }),
          ),
        ),
      }),
    );
    expect(await screen.findByText(/14 new internet addresses/i)).toBeTruthy();
    expect(screen.getByText(/3 networks never reached before/i)).toBeTruthy();
  });

  it('counts the programs a build folder hid, so they are never invisible', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              digest: { windowHours: 24, newDestinations: 0, newOwners: 0, ownerData: true, newInBuildFolders: 4 },
            }),
          ),
        ),
      }),
    );
    expect(await screen.findByText(/4 new programs appeared in your build folders/i)).toBeTruthy();
  });

  it('says nothing about build folders when none hid anything', async () => {
    show();
    await screen.findByText('New program tool.exe');
    expect(screen.queryByText(/build folders/i)).toBeNull();
  });

  it('says networks cannot be counted without the owner data', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              digest: { windowHours: 24, newDestinations: 2, newOwners: 0, ownerData: false, newInBuildFolders: 0 },
            }),
          ),
        ),
      }),
    );
    expect(await screen.findByText(/download the owner data/i)).toBeTruthy();
  });

  it('says how many open findings are new since the tab was last looked at', async () => {
    window.localStorage.setItem('syntrac.findings.lastLookedMs', String(Date.now() - 3 * 86_400_000));
    const fresh = findingRow({ id: 10, title: 'Fresh finding', firstAtMs: Date.now() - 3_600_000 });
    const old = findingRow({ id: 11, title: 'Old finding', firstAtMs: Date.now() - 10 * 86_400_000 });
    show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [fresh, old] }))) }));

    expect(await screen.findByText(/1 new since you last looked/i)).toBeTruthy();
    window.localStorage.removeItem('syntrac.findings.lastLookedMs');
  });

  it('says nothing the first time, and remembers when the tab was left', async () => {
    window.localStorage.removeItem('syntrac.findings.lastLookedMs');
    const { unmount } = render(
      <ApiProvider api={makeFakeApi()}>
        <Findings />
      </ApiProvider>,
    );
    await screen.findByText('New program tool.exe');
    expect(screen.queryByText(/since you last looked/i)).toBeNull();

    unmount();

    expect(Number(window.localStorage.getItem('syntrac.findings.lastLookedMs'))).toBeGreaterThan(Date.now() - 60_000);
    window.localStorage.removeItem('syntrac.findings.lastLookedMs');
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

  it('offers a build folder, never the whole folder, for a program in a writable tool folder', async () => {
    const inToolFolder = findingRow({ folderScope: null, buildScope: 'd:\\src' });
    const api = show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [inToolFolder] }))) }));
    await screen.findByText('New program tool.exe');
    const first = card('New program tool.exe');

    await userEvent.click(within(first).getByRole('button', { name: 'Always expected' }));
    expect(within(first).getByRole('radio', { name: /only this program/i })).toHaveProperty('checked', true);
    expect(within(first).queryByRole('radio', { name: /everything below/i })).toBeNull();
    expect(within(first).getByText(/copied there by anything else would not be called new either/i)).toBeTruthy();
    await userEvent.click(within(first).getByRole('radio', { name: /new programs in the build folder d:\\src/i }));
    await userEvent.click(within(first).getByRole('button', { name: 'Save as always expected' }));

    expect(api.decideFinding).toHaveBeenCalledWith(1, 'always_expected', { scope: 'build_folder' });
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

  it('a usage finding offers to raise the limit to what it measured instead of silencing it for ever', async () => {
    const usage = findingRow({
      id: 4,
      kind: 'usage',
      title: 'tool.exe used more CPU than usual',
      subject: 'c:\\apps\\tool.exe|cpu',
    });
    const api = show(makeFakeApi({ findings: vi.fn(async () => ok(findingsView({ findings: [usage] }))) }));
    await screen.findByText('tool.exe used more CPU than usual');
    const first = card('tool.exe used more CPU than usual');

    expect(within(first).queryByRole('button', { name: 'Always expected' })).toBeNull();
    await userEvent.click(within(first).getByRole('button', { name: 'Raise the limit to this level' }));

    expect(api.decideFinding).toHaveBeenCalledWith(4, 'raise_limit', undefined);
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

  it('says what level a raised limit allows, in the unit of what it measures', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              expectations: [
                expectationRow({ id: 7, kind: 'usage', subject: 'c:\\apps\\a.exe|cpu', note: null, ceiling: 40 }),
                expectationRow({
                  id: 8,
                  kind: 'usage',
                  subject: 'c:\\apps\\b.exe|sent',
                  note: null,
                  ceiling: 3 * 1024 ** 2,
                }),
                expectationRow({ id: 9, kind: 'usage', subject: 'c:\\apps\\c.exe|gpu', note: null, ceiling: 45 }),
                expectationRow({
                  id: 10,
                  kind: 'usage',
                  subject: 'c:\\apps\\d.exe|gpu_memory',
                  note: null,
                  ceiling: 2 * 1024 ** 3,
                }),
              ],
            }),
          ),
        ),
      }),
    );

    expect(await screen.findByText('Up to 40.0%')).toBeTruthy();
    expect(screen.getByText('Up to 3.0 MB')).toBeTruthy();
    // A GPU level is a share of one engine, not bytes (it must never read "45 B"), and GPU memory is bytes.
    expect(screen.getByText('Up to 45.0% of one GPU engine')).toBeTruthy();
    expect(screen.getByText('Up to 2.0 GB')).toBeTruthy();
  });

  it('says how wide a folder expectation is, and warns when the folder is one anything can be copied into', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              expectations: [
                expectationRow({
                  id: 20,
                  subject: 'folder:c:\\program files\\acme',
                  scope: 'folder',
                  scopeRoot: 'c:\\program files\\acme',
                }),
                expectationRow({
                  id: 21,
                  subject: 'folder:c:\\users\\someone\\.cargo',
                  scope: 'folder',
                  scopeRoot: 'c:\\users\\someone\\.cargo',
                  writableWarning: true,
                }),
                expectationRow({
                  id: 22,
                  subject: 'build:d:\\src',
                  scope: 'build',
                  scopeRoot: 'd:\\src',
                }),
              ],
            }),
          ),
        ),
      }),
    );

    const protectedRow = (await screen.findByText(/everything below c:\\program files\\acme/i)).closest('tr')!;
    expect(within(protectedRow).queryByText(/never called new/i)).toBeNull();
    const writableRow = screen.getByText(/everything below c:\\users\\someone\\.cargo/i).closest('tr')!;
    expect(within(writableRow).getByText(/a program copied there is never called new/i)).toBeTruthy();
    const buildRow = screen.getByText(/new programs in the build folder d:\\src/i).closest('tr')!;
    expect(within(buildRow).queryByText(/never called new/i)).toBeNull();
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

  it('names a known-pattern expectation by its pattern', async () => {
    show(
      makeFakeApi({
        findings: vi.fn(async () =>
          ok(
            findingsView({
              expectations: [
                expectationRow({ id: 8, kind: 'pattern_remote_access_tool', subject: 'c:\\tools\\anydesk.exe' }),
              ],
            }),
          ),
        ),
      }),
    );
    expect(await screen.findByText('Known pattern: remote access tool')).toBeTruthy();
  });
});
