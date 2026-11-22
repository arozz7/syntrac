// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { Api } from '../api/api';
import { formatDateTime } from '../format';
import { aiSettings } from '../test/fakeAi';
import { makeFakeApi } from '../test/fakeApi';
import { AiEnabledProvider } from './AiEnabled';
import { SavedNote } from './SavedNote';

afterEach(cleanup);

const WHEN = Date.UTC(2026, 10, 20, 9, 30);

function show(overrides: Partial<Api> = {}, enabled = true) {
  const api = makeFakeApi({
    aiSettings: vi.fn(async () => aiSettings({ enabled })),
    savedAnswers: vi.fn(async () => [
      { subject: { kind: 'program' as const, path: 'c:\\apps\\widget.exe' }, savedMs: WHEN },
      { subject: { kind: 'finding' as const, id: 7 }, savedMs: WHEN },
    ]),
    ...overrides,
  });
  render(
    <ApiProvider api={api}>
      <AiEnabledProvider>
        <SavedNote subject={{ kind: 'program', path: 'C:\\Apps\\Widget.exe' }} />
        <SavedNote subject={{ kind: 'finding', id: 7 }} />
        <SavedNote subject={{ kind: 'finding', id: 8 }} />
      </AiEnabledProvider>
    </ApiProvider>,
  );
  return api;
}

describe('SavedNote', () => {
  it('marks the subjects that have a kept answer, whatever the letter case of a path, and no others', async () => {
    show();
    await waitFor(() => expect(screen.getAllByText('AI note')).toHaveLength(2));
  });

  it('says in its tooltip when the answer was kept', async () => {
    show();
    const notes = await screen.findAllByText('AI note');
    expect(notes[0]?.getAttribute('title')).toContain(formatDateTime(WHEN));
  });

  it('shows nothing, and asks for nothing, while the AI help is turned off', async () => {
    const api = show({}, false);
    await waitFor(() => expect(api.aiSettings).toHaveBeenCalled());
    expect(screen.queryByText('AI note')).toBeNull();
    expect(api.savedAnswers).not.toHaveBeenCalled();
  });
});
