// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { Api } from '../api/api';
import type { InvestigateSubject } from '../api/aiTypes';
import { aiSettings, investigation, promptPreview, savedAnswer } from '../test/fakeAi';
import { formatDateTime } from '../format';
import { makeFakeApi } from '../test/fakeApi';
import { AiEnabledProvider } from './AiEnabled';
import { InvestigateButton } from './InvestigatePanel';

afterEach(cleanup);

const SUBJECT: InvestigateSubject = { kind: 'finding', id: 7 };

function show(overrides: Partial<Api> = {}, enabled = true) {
  const api = makeFakeApi({ aiSettings: vi.fn(async () => aiSettings({ enabled })), ...overrides });
  render(
    <ApiProvider api={api}>
      <AiEnabledProvider>
        <InvestigateButton subject={SUBJECT} />
      </AiEnabledProvider>
    </ApiProvider>,
  );
  return api;
}

async function open() {
  await userEvent.click(await screen.findByRole('button', { name: 'Investigate' }));
}

describe('InvestigateButton', () => {
  it('shows nothing at all while the AI help is turned off', async () => {
    const api = show({}, false);
    await waitFor(() => expect(api.aiSettings).toHaveBeenCalled());
    expect(screen.queryByRole('button', { name: 'Investigate' })).toBeNull();
  });

  it('asks nothing until a button is pressed', async () => {
    const api = show();
    await open();
    expect(api.startInvestigation).not.toHaveBeenCalled();
    expect(api.previewPrompt).not.toHaveBeenCalled();
    expect(screen.getByText(/nothing leaves your computer/i)).toBeTruthy();
  });

  it('starts an investigation on the subject and shows the answer as plain text with what was sent', async () => {
    const api = show({
      pollInvestigation: vi.fn(async () =>
        investigation({ answer: '<b>not bold</b>\nsecond line', sent: '<evidence>{"x":1}</evidence>' }),
      ),
    });
    await open();

    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));

    expect(api.startInvestigation).toHaveBeenCalledWith(SUBJECT);
    const answer = await screen.findByLabelText('Answer');
    expect(answer.textContent).toBe('<b>not bold</b>\nsecond line');
    expect(answer.querySelector('b')).toBeNull();
    expect(screen.getByText(/can be wrong or incomplete/i)).toBeTruthy();
    expect(screen.getByText(/What was sent \(summary detail\)/)).toBeTruthy();
  });

  it('says why when the investigation cannot start', async () => {
    show({
      startInvestigation: vi.fn(async () => ({ ok: false as const, message: 'The model server is not running.' })),
    });
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));
    expect((await screen.findByRole('alert')).textContent).toContain('not running');
  });

  it('shows a failure the server reported and keeps what arrived', async () => {
    show({
      pollInvestigation: vi.fn(async () =>
        investigation({ phase: 'failed', answer: 'partial', error: 'The model server did not answer in time.' }),
      ),
    });
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));
    expect((await screen.findByRole('alert')).textContent).toContain('did not answer in time');
    expect(screen.getByLabelText('Answer').textContent).toBe('partial');
  });

  it('offers Stop while it runs and tells the program to cancel', async () => {
    const api = show({
      pollInvestigation: vi.fn(async () => investigation({ id: 1, phase: 'running', answer: 'so far' })),
    });
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Stop' }));
    expect(api.cancelInvestigation).toHaveBeenCalledWith(1);
  });

  it('prepares an editable text for an outside assistant, says it shares nothing itself, and copies what is in the box', async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const api = show({
      previewPrompt: vi.fn(async () => ({ ok: true as const, value: promptPreview({ text: 'REDUCED TEXT' }) })),
    });
    await open();
    expect(screen.getByText(/sends nothing anywhere/i)).toBeTruthy();

    await userEvent.click(screen.getByRole('button', { name: 'Show the text to copy' }));

    expect(api.previewPrompt).toHaveBeenCalledWith(SUBJECT, true);
    const box = await screen.findByLabelText('Exactly what will be copied');
    expect((box as HTMLTextAreaElement).value).toBe('REDUCED TEXT');
    await userEvent.type(box, ' + my note');
    await userEvent.click(screen.getByRole('button', { name: 'Copy to clipboard' }));
    expect(writeText).toHaveBeenCalledWith('REDUCED TEXT + my note');
    expect(await screen.findByText('Copied.')).toBeTruthy();
  });

  it('says so when the clipboard cannot be used', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      value: {
        writeText: vi.fn(async () => {
          throw new Error('denied');
        }),
      },
      configurable: true,
    });
    show();
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Show the text to copy' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Copy to clipboard' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Could not copy');
  });
});

describe('InvestigateButton: a model that thinks first, and an answer that is cut off', () => {
  it('says the model is thinking, and how much, while it has not yet produced any answer', async () => {
    show({
      pollInvestigation: vi.fn(async () =>
        investigation({ id: 1, phase: 'running', answer: '', reasoningChars: 1234 }),
      ),
    });
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));
    expect(await screen.findByText(/The model is thinking/)).toBeTruthy();
    expect(screen.getByText(/1234 characters/)).toBeTruthy();
    expect(screen.getByText(/minute or two before its first words/)).toBeTruthy();
  });

  it('does not call an answer finished when the model stopped at its length limit', async () => {
    show({
      pollInvestigation: vi.fn(async () => investigation({ phase: 'done', answer: 'The program is', truncated: true })),
    });
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));
    expect(await screen.findByText('Finished, but cut off.')).toBeTruthy();
    expect(screen.getByText(/reached its length limit/)).toBeTruthy();
    expect(screen.queryByText('Finished.')).toBeNull();
  });

  it('says nothing about a cut-off for an answer that finished normally', async () => {
    show();
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Investigate with my model' }));
    expect(await screen.findByText('Finished.')).toBeTruthy();
    expect(screen.queryByText(/length limit/)).toBeNull();
  });
});

describe('InvestigateButton: answers kept from an earlier time', () => {
  const kept = savedAnswer({ subject: SUBJECT, answer: 'Kept: a sync tool.', model: 'big-model' });

  it('shows the kept answer with when it was kept, and asks nothing', async () => {
    const api = show({ savedAnswer: vi.fn(async () => kept) });
    await open();

    expect((await screen.findByLabelText('Answer')).textContent).toBe('Kept: a sync tool.');
    expect(api.savedAnswer).toHaveBeenCalledWith(SUBJECT);
    expect(screen.getByText(new RegExp(`Saved answer from ${formatDateTime(kept.savedMs)}`))).toBeTruthy();
    expect(screen.getByText(/big-model/)).toBeTruthy();
    expect(api.startInvestigation).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Investigate with my model' })).toBeNull();
    expect(screen.getByText(/What was sent \(summary detail\)/)).toBeTruthy();
  });

  it('says a kept answer was cut off', async () => {
    show({ savedAnswer: vi.fn(async () => ({ ...kept, truncated: true })) });
    await open();
    expect(await screen.findByText(/reached its length limit/)).toBeTruthy();
  });

  it('shows no saved-answer line when nothing was kept', async () => {
    show();
    await open();
    await screen.findByRole('button', { name: 'Investigate with my model' });
    expect(screen.queryByText(/Saved answer from/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Forget the saved answer' })).toBeNull();
  });

  it('asks the model again only when told to, and then shows the new answer instead', async () => {
    const api = show({
      savedAnswer: vi.fn(async () => kept),
      pollInvestigation: vi.fn(async () => investigation({ answer: 'A fresh answer.' })),
    });
    await open();
    await userEvent.click(await screen.findByRole('button', { name: 'Ask again' }));

    expect(api.startInvestigation).toHaveBeenCalledWith(SUBJECT);
    await waitFor(() => expect(screen.getByLabelText('Answer').textContent).toBe('A fresh answer.'));
    expect(screen.queryByText(/Saved answer from/)).toBeNull();
  });

  it('forgets the kept answer on request and goes back to the first state', async () => {
    const api = show({ savedAnswer: vi.fn(async () => kept) });
    await open();

    await userEvent.click(await screen.findByRole('button', { name: 'Forget the saved answer' }));

    expect(api.forgetAnswer).toHaveBeenCalledWith(SUBJECT);
    expect(await screen.findByRole('button', { name: 'Investigate with my model' })).toBeTruthy();
    expect(screen.queryByLabelText('Answer')).toBeNull();
  });

  it('reads the kept answer again, and refreshes the marks in the lists, when a new answer finishes', async () => {
    const api = show();
    await open();
    await userEvent.click(await screen.findByRole('button', { name: 'Investigate with my model' }));

    await screen.findByText('Finished.');

    await waitFor(() => expect(vi.mocked(api.savedAnswer).mock.calls.length).toBeGreaterThanOrEqual(2));
    await waitFor(() => expect(vi.mocked(api.savedAnswers).mock.calls.length).toBeGreaterThanOrEqual(2));
  });
});
