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
  /** The folder the command runs in; null when none is set (the program then runs in SYNTRAC's own folder). */
  workingFolder: string | null;
  /** Where what the server prints is saved, when this app started one. */
  logPath: string | null;
  /** The last lines the server printed, when it ended or is not answering. */
  logTail: string[];
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
  /** The answer stopped at the length limit and is cut off, not finished. */
  truncated: boolean;
  error: string | null;
  /** The evidence block that was sent, so the answer can be checked against it. */
  sent: string;
  level: string;
}

/** An answer kept from an earlier investigation, so the same subject need not be asked about again. */
export interface SavedAnswer {
  subject: InvestigateSubject;
  answer: string;
  /** The answer was cut off at the model's length limit. */
  truncated: boolean;
  model: string;
  level: string;
  /** The evidence block that was sent. */
  sent: string;
  /** When it was kept, in milliseconds since 1970. */
  savedMs: number;
}

/** That an answer is kept for a subject and since when, without the answer. */
export interface SavedMark {
  subject: InvestigateSubject;
  savedMs: number;
}

/** A command that can refuse says why, in words for the person. */
export type AiResult<T> = { ok: true; value: T } | { ok: false; message: string };

// ---- The question box (docs/PHASE5-PLAN.md, B). They mirror the views in app/src-tauri/src/question_job.rs.

/** One tool call the model made while answering a question. */
export interface QuestionCall {
  name: string;
  /** The arguments as asked, cleaned and cut. */
  arguments: string;
  /** `done`, `refused` (not run) or `failed`. */
  status: 'done' | 'refused' | 'failed';
  note: string | null;
  resultBytes: number;
  /** The result was cut to fit the model's small window. */
  cut: boolean;
}

export type QuestionStop = 'answered' | 'call_limit' | 'time_budget' | 'context_full' | 'cancelled';

export interface Question {
  id: number;
  phase: InvestigationPhase;
  question: string;
  answer: string;
  reasoningChars: number;
  truncated: boolean;
  error: string | null;
  /** The server cannot do tool calls; `error` says what to do. */
  toolsUnsupported: boolean;
  /** Every tool call so far, oldest first, so the answer can be checked against what was looked up. */
  calls: QuestionCall[];
  stopped: QuestionStop | null;
  level: string;
}
