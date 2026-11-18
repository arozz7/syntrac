// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import { makeFakeApi, reviewGroup, reviewView } from '../test/fakeApi';
import { ReviewToday } from './ReviewToday';

afterEach(cleanup);

function show(api = makeFakeApi(), onChanged: () => void = () => undefined) {
  render(
    <ApiProvider api={api}>
      <ReviewToday onChanged={onChanged} />
    </ApiProvider>,
  );
  return api;
}

const open = async () => userEvent.click(screen.getByRole('button', { name: /review what was here/i }));
const group = (label: string) => screen.getByRole('heading', { name: label }).closest('article')!;

describe('ReviewToday', () => {
  it('reads nothing until it is opened', async () => {
    const api = show();
    expect(api.review).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: /review what was here/i }).getAttribute('aria-expanded')).toBe('false');

    await open();

    expect(await screen.findByText('Acme Suite')).toBeTruthy();
    expect(api.review).toHaveBeenCalled();
  });

  it('lists the groups with their size, where they live and what deserves a look, and the names of some programs', async () => {
    show();
    await open();
    await screen.findByText('Acme Suite');

    const looked = group('downloads');
    expect(within(looked).getByText(/it is not signed and it runs from a user folder/i)).toBeTruthy();
    expect(within(looked).getByText('tool.exe')).toBeTruthy();
    const quiet = group('Acme Suite');
    expect(within(quiet).getByText(/3 programs/)).toBeTruthy();
    expect(within(quiet).getByText(/\+1 more/)).toBeTruthy();
    expect(within(quiet).queryByText(/worth a look/i)).toBeNull();
  });

  it('"Mark as expected" asks for the whole install folder and reads the review again', async () => {
    const changed = vi.fn();
    const api = show(makeFakeApi(), changed);
    await open();
    await screen.findByText('Acme Suite');
    const before = api.review.mock.calls.length;

    await userEvent.click(within(group('Acme Suite')).getByRole('button', { name: 'Mark as expected' }));

    expect(api.confirmReview).toHaveBeenCalledWith('c:\\program files\\acme');
    await waitFor(() => expect(api.review.mock.calls.length).toBeGreaterThan(before));
    expect(changed).toHaveBeenCalled();
  });

  it('a folder anything can be copied into is expected program by program, never as a whole folder', async () => {
    const changed = vi.fn();
    const api = show(makeFakeApi(), changed);
    await open();
    await screen.findByText('Acme Suite');
    const before = api.review.mock.calls.length;
    const looked = group('downloads');

    expect(within(looked).queryByRole('button', { name: 'Mark as expected' })).toBeNull();
    await userEvent.click(within(looked).getByRole('button', { name: 'Mark this program as expected' }));

    expect(api.confirmReviewPrograms).toHaveBeenCalledWith('c:\\users\\someone\\downloads', null);
    expect(api.confirmReview).not.toHaveBeenCalled();
    await waitFor(() => expect(api.review.mock.calls.length).toBeGreaterThan(before));
    expect(changed).toHaveBeenCalled();
  });

  it('names how many programs a group of several will mark, and says a later program is still raised', async () => {
    const many = reviewGroup({
      root: 'd:\\src',
      label: 'src',
      publisher: 'maker inc',
      count: 12,
      wholeFolderAllowed: false,
      location: 'another drive',
    });
    const api = show(
      makeFakeApi({
        review: vi.fn(async () => ({ state: 'ok' as const, data: reviewView({ groups: [many], totalGroups: 1 }) })),
      }),
    );
    await open();
    await screen.findByText('src');

    expect(within(group('src')).getByText(/a program that appears later is still raised/i)).toBeTruthy();
    await userEvent.click(within(group('src')).getByRole('button', { name: 'Mark these 12 programs as expected' }));

    expect(api.confirmReviewPrograms).toHaveBeenCalledWith('d:\\src', 'maker inc');
  });

  it('"Mark as a known program" uses the first program of the group and the group name', async () => {
    const api = show();
    await open();
    await screen.findByText('Acme Suite');

    await userEvent.click(within(group('Acme Suite')).getByRole('button', { name: 'Mark as a known program' }));

    expect(api.markKnown).toHaveBeenCalledWith('C:\\Program Files\\Acme\\a.exe', 'Acme Suite');
  });

  it('shows why a change was refused and leaves the group where it was', async () => {
    const api = makeFakeApi({
      confirmReview: vi.fn(async () => ({ ok: false, id: null, message: 'The collector is not running.' })),
    });
    show(api);
    await open();
    await screen.findByText('Acme Suite');

    await userEvent.click(within(group('Acme Suite')).getByRole('button', { name: 'Mark as expected' }));

    expect((await within(group('Acme Suite')).findByRole('alert')).textContent).toMatch(/not running/);
  });

  it('says plainly when there is nothing to review, and when detection has not been switched on', async () => {
    show(
      makeFakeApi({
        review: vi.fn(async () => ({ state: 'ok' as const, data: reviewView({ groups: [], totalGroups: 0 }) })),
      }),
    );
    await open();
    expect(await screen.findByText(/nothing left to review/i)).toBeTruthy();

    cleanup();
    show(
      makeFakeApi({ review: vi.fn(async () => ({ state: 'ok' as const, data: reviewView({ switchedOnMs: null }) })) }),
    );
    await open();
    expect(await screen.findByText(/has not been switched on yet/i)).toBeTruthy();
  });

  it('says how many groups there are when only some are listed', async () => {
    show(
      makeFakeApi({ review: vi.fn(async () => ({ state: 'ok' as const, data: reviewView({ totalGroups: 150 }) })) }),
    );
    await open();
    expect(await screen.findByText(/showing 2 of 150 groups/i)).toBeTruthy();
  });
});
