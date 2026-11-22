// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { Api } from '../api/api';
import { aiSettings, question } from '../test/fakeAi';
import { makeFakeApi } from '../test/fakeApi';
import { AiEnabledProvider } from './AiEnabled';
import { QuestionBox } from './QuestionBox';

afterEach(cleanup);

function show(overrides: Partial<Api> = {}, enabled = true) {
  const api = makeFakeApi({ aiSettings: vi.fn(async () => aiSettings({ enabled })), ...overrides });
  render(
    <ApiProvider api={api}>
      <AiEnabledProvider>
        <QuestionBox />
      </AiEnabledProvider>
    </ApiProvider>,
  );
  return api;
}

async function ask(text = 'What is widgetsync?') {
  await userEvent.type(await screen.findByLabelText('Your question'), text);
  await userEvent.click(screen.getByRole('button', { name: 'Ask' }));
}

describe('QuestionBox', () => {
  it('shows nothing at all while the AI help is turned off', async () => {
    const api = show({}, false);
    await waitFor(() => expect(api.aiSettings).toHaveBeenCalled());
    expect(screen.queryByLabelText('Your question')).toBeNull();
  });

  it('asks nothing until Ask is pressed, and Ask needs a question', async () => {
    const api = show();
    const button = await screen.findByRole('button', { name: 'Ask' });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    await userEvent.type(screen.getByLabelText('Your question'), '   ');
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(api.askQuestion).not.toHaveBeenCalled();
  });

  it('sends the question and shows the answer as plain text', async () => {
    const api = show({ pollQuestion: vi.fn(async () => question({ answer: '<b>not bold</b>\nsecond line' })) });

    await ask('What is widgetsync?');

    expect(api.askQuestion).toHaveBeenCalledWith('What is widgetsync?');
    const answer = await screen.findByLabelText('Answer');
    expect(answer.textContent).toBe('<b>not bold</b>\nsecond line');
    expect(answer.querySelector('b')).toBeNull();
    expect(screen.getByText(/can be wrong or incomplete/i)).toBeTruthy();
  });

  it('lists every tool call that was made, so the answer can be checked against it', async () => {
    show({
      pollQuestion: vi.fn(async () =>
        question({
          calls: [
            { name: 'top_programs', arguments: '{}', status: 'done', note: null, resultBytes: 900, cut: false },
            {
              name: 'delete_all',
              arguments: '{}',
              status: 'refused',
              note: 'there is no tool by that name',
              resultBytes: 80,
              cut: false,
            },
            {
              name: 'program_facts',
              arguments: '{"path":"x"}',
              status: 'done',
              note: null,
              resultBytes: 3000,
              cut: true,
            },
          ],
        }),
      ),
    });
    await ask();

    const list = await screen.findByRole('list', { name: 'What was looked up' });
    const items = within(list).getAllByRole('listitem');
    expect(items).toHaveLength(3);
    expect(items[0]?.textContent).toContain('top_programs');
    expect(items[1]?.textContent).toMatch(/delete_all.*not run.*no tool by that name/);
    expect(items[2]?.textContent).toMatch(/program_facts.*cut/);
  });

  it('says when the model looked nothing up', async () => {
    show({ pollQuestion: vi.fn(async () => question({ calls: [] })) });
    await ask();
    expect(await screen.findByText(/looked nothing up/i)).toBeTruthy();
  });

  it('says why the box could not start', async () => {
    show({ askQuestion: vi.fn(async () => ({ ok: false as const, message: 'The model server is not running.' })) });
    await ask();
    expect((await screen.findByRole('alert')).textContent).toContain('not running');
  });

  it('explains a server that cannot do tool calls', async () => {
    show({
      pollQuestion: vi.fn(async () =>
        question({
          phase: 'failed',
          answer: '',
          calls: [],
          stopped: null,
          toolsUnsupported: true,
          error: 'The model server did not accept tool calls. Use a server that supports them.',
        }),
      ),
    });
    await ask();
    expect((await screen.findByRole('alert')).textContent).toContain('did not accept tool calls');
  });

  it('offers Stop while it runs and tells the program to cancel', async () => {
    const api = show({
      pollQuestion: vi.fn(async () => question({ id: 1, phase: 'running', answer: '', stopped: null })),
    });
    await ask();
    await userEvent.click(await screen.findByRole('button', { name: 'Stop' }));
    expect(api.cancelQuestion).toHaveBeenCalledWith(1);
  });

  it('says why it stopped when it ended before the model chose to answer', async () => {
    for (const [stopped, text] of [
      ['call_limit', /limit of tool calls/i],
      ['time_budget', /time/i],
      ['context_full', /window/i],
    ] as const) {
      const view = show({ pollQuestion: vi.fn(async () => question({ stopped })) });
      await ask();
      expect(await screen.findByText(text)).toBeTruthy();
      cleanup();
      expect(view.pollQuestion).toHaveBeenCalled();
    }
  });

  it('says a cut-off answer is cut off', async () => {
    show({ pollQuestion: vi.fn(async () => question({ truncated: true })) });
    await ask();
    expect(await screen.findByText(/length limit/i)).toBeTruthy();
  });

  it('says how thinking models are waited for', async () => {
    show({
      pollQuestion: vi.fn(async () => question({ phase: 'running', answer: '', reasoningChars: 120, stopped: null })),
    });
    await ask();
    expect(await screen.findByText(/thinking/i)).toBeTruthy();
  });
});
