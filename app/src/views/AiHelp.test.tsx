// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiProvider } from '../api/ApiContext';
import type { Api } from '../api/api';
import { AiEnabledProvider } from '../components/AiEnabled';
import { aiSettings, aiStatus } from '../test/fakeAi';
import { makeFakeApi } from '../test/fakeApi';
import { AiHelp } from './AiHelp';

afterEach(cleanup);

function show(overrides: Partial<Api> = {}) {
  const api = makeFakeApi(overrides);
  render(
    <ApiProvider api={api}>
      <AiEnabledProvider>
        <AiHelp />
      </AiEnabledProvider>
    </ApiProvider>,
  );
  return api;
}

describe('AiHelp', () => {
  it('starts off, says it is optional and local, and shows nothing running', async () => {
    show();
    expect(await screen.findByLabelText('Turn on AI help')).toBeTruthy();
    expect((screen.getByLabelText('Turn on AI help') as HTMLInputElement).checked).toBe(false);
    expect(screen.getByText(/never to the Internet/i)).toBeTruthy();
    expect(await screen.findByText('The model server is not running.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Start the model server' })).toBeNull();
  });

  it('saves what was typed, with the arguments one per line', async () => {
    const api = show();
    await userEvent.click(await screen.findByLabelText('Turn on AI help'));
    await userEvent.type(screen.getByLabelText(/Model server address/), 'http://127.0.0.1:1234/v1');
    await userEvent.type(screen.getByLabelText(/^Model name/), 'some-model');
    await userEvent.type(screen.getByLabelText(/Program \(a full path/), 'C:\\Tools\\srv.exe');
    await userEvent.type(screen.getByLabelText(/Arguments/), 'serve{Enter}--port 1234');
    await userEvent.click(screen.getByLabelText(/Full: also addresses/));

    await userEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(api.saveAiSettings).toHaveBeenCalledWith(
      aiSettings({
        enabled: true,
        endpoint: 'http://127.0.0.1:1234/v1',
        model: 'some-model',
        detailLevel: 'full',
        launchProgram: 'C:\\Tools\\srv.exe',
        launchArgs: ['serve', '--port 1234'],
      }),
    );
    expect(await screen.findByText('Saved.')).toBeTruthy();
  });

  it('shows the reason when the settings are refused', async () => {
    show({
      saveAiSettings: vi.fn(async () => ({
        ok: false as const,
        message: 'the address is not this machine (use 127.0.0.1, localhost or [::1])',
      })),
    });
    await userEvent.type(await screen.findByLabelText(/Model server address/), 'http://example.com/v1');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect((await screen.findByRole('alert')).textContent).toContain('not this machine');
  });

  it('shows the exact command and starts the server only when the button is pressed', async () => {
    const api = show({
      aiSettings: vi.fn(async () => aiSettings({ enabled: true, launchProgram: 'C:\\Tools\\srv.exe' })),
      aiStatus: vi.fn(async () =>
        aiStatus({
          canStart: true,
          command: '"C:\\Tools\\srv.exe" serve',
          message: 'The model server is not running.',
        }),
      ),
    });
    expect(await screen.findByText('"C:\\Tools\\srv.exe" serve')).toBeTruthy();
    expect(api.startAiServer).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('button', { name: 'Start the model server' }));

    expect(api.startAiServer).toHaveBeenCalledTimes(1);
  });

  it('offers Stop only for a server the program started and says why a start failed', async () => {
    const api = show({
      aiStatus: vi.fn(async () =>
        aiStatus({
          state: 'ready',
          message: 'The model server is ready (started by this app).',
          canStop: true,
          startedByThisApp: true,
        }),
      ),
      stopAiServer: vi.fn(async () => ({
        ok: false as const,
        message: 'The server could not be stopped: access denied.',
      })),
    });
    await userEvent.click(await screen.findByRole('button', { name: 'Stop the model server' }));
    await waitFor(() => expect(api.stopAiServer).toHaveBeenCalled());
    expect((await screen.findByRole('alert')).textContent).toContain('access denied');
    expect(screen.getByText(/only stops a server it started itself/i)).toBeTruthy();
  });
});

describe('AiHelp: why a server did not start', () => {
  it('shows the working folder the command runs in, or says there is none', async () => {
    show({
      aiStatus: vi.fn(async () =>
        aiStatus({ canStart: true, command: '"C:\\Tools\\srv.exe" serve', workingFolder: 'C:\\Tools' }),
      ),
    });
    expect(await screen.findByText('C:\\Tools')).toBeTruthy();
    expect(screen.queryByText(/No working folder is set/)).toBeNull();
    cleanup();

    show({
      aiStatus: vi.fn(async () =>
        aiStatus({ canStart: true, command: '"C:\\Tools\\srv.exe" serve', workingFolder: null }),
      ),
    });
    expect(await screen.findByText(/No working folder is set/)).toBeTruthy();
  });

  it('shows what the server printed last, as plain text, and where the log is, when it has ended', async () => {
    show({
      aiStatus: vi.fn(async () =>
        aiStatus({
          state: 'exited',
          message: 'The server this app started has ended with code 1.',
          canStart: true,
          command: '"C:\\Tools\\srv.exe" serve',
          logTail: ['starting', 'Error: <b>failed</b> to start'],
          logPath: 'C:\\Users\\x\\AppData\\Local\\Syntrac\\model-server.log',
        }),
      ),
    });
    const printed = await screen.findByLabelText('What the server printed');
    expect(printed.textContent).toBe('starting\nError: <b>failed</b> to start');
    expect(printed.querySelector('b')).toBeNull();
    expect(screen.getByText(/model-server\.log/)).toBeTruthy();
  });
});

describe('AiHelp: the Start button answers at once', () => {
  it('says it is starting the moment it is pressed, before the server has answered anything', async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const api = show({
      aiStatus: vi.fn(async () => aiStatus({ canStart: true, command: '"C:\\Tools\\srv.exe" serve' })),
      startAiServer: vi.fn(async () => {
        await held;
        return { ok: true as const, value: null };
      }),
    });

    await userEvent.click(await screen.findByRole('button', { name: 'Start the model server' }));

    // Nothing has come back from the program yet, and the page already says so.
    expect(await screen.findByText(/Starting the model server/)).toBeTruthy();
    expect(screen.getByText(/can take several minutes/)).toBeTruthy();
    const busy = screen.getByRole('button', { name: 'Starting…' }) as HTMLButtonElement;
    expect(busy.disabled).toBe(true);
    expect(api.startAiServer).toHaveBeenCalledTimes(1);
    release();
  });

  it('goes back to the normal state and says why when the start is refused', async () => {
    show({
      aiStatus: vi.fn(async () => aiStatus({ canStart: true, command: '"C:\\Tools\\srv.exe" serve' })),
      startAiServer: vi.fn(async () => ({ ok: false as const, message: 'The program file was not found.' })),
    });
    await userEvent.click(await screen.findByRole('button', { name: 'Start the model server' }));
    expect((await screen.findByRole('alert')).textContent).toContain('not found');
    expect(screen.queryByText(/Starting the model server/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Start the model server' })).toBeTruthy();
  });

  it('stops showing "starting" once the program reports that the server is starting', async () => {
    let started = false;
    show({
      aiStatus: vi.fn(async () =>
        started
          ? aiStatus({
              state: 'starting',
              message: 'Starting. A large model can take several minutes to load.',
              canStop: true,
              startedByThisApp: true,
            })
          : aiStatus({ canStart: true, command: '"C:\\Tools\\srv.exe" serve' }),
      ),
      startAiServer: vi.fn(async () => {
        started = true;
        return { ok: true as const, value: null };
      }),
    });
    await userEvent.click(await screen.findByRole('button', { name: 'Start the model server' }));
    expect(await screen.findByRole('button', { name: 'Stop the model server' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Starting…' })).toBeNull();
  });
});
