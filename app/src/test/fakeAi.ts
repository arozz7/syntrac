import { vi } from 'vitest';
import type { Api } from '../api/api';
import type { AiSettings, AiStatus, Investigation, PromptPreview } from '../api/aiTypes';

/** The AI help as a new installation has it: off and empty. */
export function aiSettings(patch: Partial<AiSettings> = {}): AiSettings {
  return {
    enabled: false,
    endpoint: '',
    model: '',
    detailLevel: 'summary',
    launchProgram: '',
    launchArgs: [],
    launchWorkingDir: '',
    startOnInvestigate: false,
    stopOnQuit: false,
    ...patch,
  };
}

export function aiStatus(patch: Partial<AiStatus> = {}): AiStatus {
  return {
    state: 'not_running',
    message: 'The model server is not running.',
    seconds: null,
    startedByThisApp: false,
    canStart: false,
    canStop: false,
    command: null,
    ...patch,
  };
}

export function investigation(patch: Partial<Investigation> = {}): Investigation {
  return {
    id: 1,
    phase: 'done',
    answer: 'It looks like a sync tool.',
    reasoningChars: 0,
    error: null,
    sent: '<evidence>\n{ "detail_level": "summary" }\n</evidence>',
    level: 'summary',
    ...patch,
  };
}

export function promptPreview(patch: Partial<PromptPreview> = {}): PromptPreview {
  return {
    level: 'external',
    system: 'You help a person understand activity.',
    user: 'Using only the evidence below…\n\n<evidence>\n{}\n</evidence>',
    text: 'You help a person understand activity.\n\nUsing only the evidence below…\n\n<evidence>\n{}\n</evidence>',
    ...patch,
  };
}

/** Defaults for the AI part of the Api: everything off, nothing running. */
export function fakeAiApi(): Pick<
  Api,
  | 'aiSettings'
  | 'saveAiSettings'
  | 'aiStatus'
  | 'startAiServer'
  | 'stopAiServer'
  | 'previewPrompt'
  | 'startInvestigation'
  | 'pollInvestigation'
  | 'cancelInvestigation'
> {
  return {
    aiSettings: vi.fn(async () => aiSettings()),
    saveAiSettings: vi.fn(async (settings: AiSettings) => ({ ok: true as const, value: settings })),
    aiStatus: vi.fn(async () => aiStatus()),
    startAiServer: vi.fn(async () => ({ ok: true as const, value: null })),
    stopAiServer: vi.fn(async () => ({ ok: true as const, value: null })),
    previewPrompt: vi.fn(async () => ({ ok: true as const, value: promptPreview() })),
    startInvestigation: vi.fn(async () => ({ ok: true as const, value: 1 })),
    pollInvestigation: vi.fn(async () => investigation()),
    cancelInvestigation: vi.fn(async () => undefined),
  };
}
