// Types of the optional AI help (docs/PHASE4-PLAN.md). They mirror the views in app/src-tauri/src/ai_control.rs and ai_investigate.rs.

export type AiDetailLevel = 'summary' | 'full';

export interface AiSettings {
  enabled: boolean;
  /** For example `http://127.0.0.1:1234/v1`; only an address on this machine is accepted. */
  endpoint: string;
  /** Sent to the server exactly as typed. */
  model: string;
  detailLevel: AiDetailLevel;
  /** Optional: how to start the model server from the app. Empty means the app does not manage the server. */
  launchProgram: string;
  launchArgs: string[];
  launchWorkingDir: string;
  startOnInvestigate: boolean;
  stopOnQuit: boolean;
}

export type AiServerState =
  'off' | 'not_set_up' | 'not_running' | 'starting' | 'slow_to_start' | 'loading' | 'ready' | 'exited' | 'failed';

export interface AiStatus {
  state: AiServerState;
  message: string;
  seconds: number | null;
  startedByThisApp: boolean;
  canStart: boolean;
  canStop: boolean;
  /** The exact command a Start would run, when one is set and allowed. */
  command: string | null;
}

/** What an investigation is about. */
export type InvestigateSubject = { kind: 'finding'; id: number } | { kind: 'program'; path: string };

export interface PromptPreview {
  /** `summary`, `full` or `external`. */
  level: string;
  system: string;
  user: string;
  /** Everything in one text, for pasting into an assistant that has no separate instruction field. */
  text: string;
}

export type InvestigationPhase = 'waiting' | 'running' | 'done' | 'failed' | 'cancelled';

export interface Investigation {
  id: number;
  phase: InvestigationPhase;
  /** The answer so far, as plain text. */
  answer: string;
  reasoningChars: number;
  error: string | null;
  /** The evidence block that was sent, so the answer can be checked against it. */
  sent: string;
  level: string;
}

/** A command that can refuse says why, in words for the person. */
export type AiResult<T> = { ok: true; value: T } | { ok: false; message: string };
