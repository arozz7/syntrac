import { invoke } from '@tauri-apps/api/core';
import type {
  AiResult,
  AiSettings,
  AiStatus,
  InvestigateSubject,
  Investigation,
  PromptPreview,
  SavedAnswer,
  SavedMark,
} from './aiTypes';
import type {
  ConnectionRowView,
  Data,
  DestinationHistoryView,
  DestinationOrder,
  DestinationsView,
  HealthView,
  KnownProgramsView,
  NewDestinationsView,
  OwnersView,
  FindingDecision,
  FindingStatus,
  FindingsView,
  ReviewView,
  ProgramDestinationsView,
  OverviewView,
  OwnerDataOutcome,
  ProcessListParams,
  ProcessRowView,
  ProcessTreeView,
  ProgramHistoryView,
  ProgramMetric,
  ProgramsView,
  SystemHistoryView,
  WriteOutcome,
} from './types';

/** Everything the UI can ask of the backend. Components depend on this interface, never on Tauri directly. */
export interface Api {
  health(): Promise<HealthView>;
  overview(rangeSecs: number): Promise<Data<OverviewView>>;
  /** The machine hour by hour from the rollups, which outlive the detail. */
  systemHistory(rangeSecs: number): Promise<Data<SystemHistoryView>>;
  /** The programs that used the most of `metric` over the range, from the rollups. */
  topPrograms(
    rangeSecs: number,
    metric: ProgramMetric,
    limit?: number,
    groupByProduct?: boolean,
  ): Promise<Data<ProgramsView>>;
  /** One program, hour by hour. `path` is the program's image path as listed by `topPrograms`. */
  programHistory(rangeSecs: number, path: string): Promise<Data<ProgramHistoryView>>;
  /** Who started whom among the processes alive in the last `windowSecs` (detail is kept for 14 days). */
  processTree(windowSecs: number): Promise<Data<ProcessTreeView>>;
  /** Where the machine talked to the most over the range, from the rollups. Addresses on this network and multicast are left out unless `allAddresses`. */
  topDestinations(
    rangeSecs: number,
    order: DestinationOrder,
    limit?: number,
    allAddresses?: boolean,
  ): Promise<Data<DestinationsView>>;
  /** The same destinations grouped by the network that owns them, biggest owner first. */
  topOwners(rangeSecs: number, limit?: number, allAddresses?: boolean): Promise<Data<OwnersView>>;
  /** The findings with this status (every status when none), the counts, what the user said is expected, and whether detection is on. */
  findings(status?: FindingStatus): Promise<Data<FindingsView>>;
  /** Asks the service to apply the user's decision. `days` is for a snooze, `note` for "always expected". Never throws: a refusal is in the outcome. */
  decideFinding(
    id: number,
    decision: FindingDecision,
    options?: { days?: number; note?: string; scope?: 'this_item' | 'this_folder' | 'build_folder' },
  ): Promise<WriteOutcome>;
  /** The groups of programs that were already here when detection was switched on and that nothing covers yet. */
  review(): Promise<Data<ReviewView>>;
  /** Asks the service to expect new programs below an install folder the user has looked at (a protected folder only). Never throws. */
  confirmReview(root: string): Promise<WriteOutcome>;
  /** Asks the service to expect the programs of one review group, one by one (never the folder). Never throws. */
  confirmReviewPrograms(root: string, publisher: string | null): Promise<WriteOutcome>;
  /** Whether Windows notifications for new suspicious findings are on. The PROGRAM sends them (with or without a window); this is its switch. */
  notificationsOn(): Promise<boolean>;
  /** Switches the notifications (and the tray menu's check mark). Never throws. */
  setNotificationsOn(on: boolean): Promise<void>;
  /** Asks the service to remove something the user said is expected. Never throws. */
  removeExpectation(id: number): Promise<WriteOutcome>;
  /** Where one program connected over the range, biggest first. `path` is the program's image path. */
  programDestinations(
    rangeSecs: number,
    path: string,
    limit?: number,
    allAddresses?: boolean,
  ): Promise<Data<ProgramDestinationsView>>;
  /** Destinations first seen within the last `windowSecs` (never counting the first day of history as new). */
  newDestinations(windowSecs: number, limit?: number, allAddresses?: boolean): Promise<Data<NewDestinationsView>>;
  /** One destination: the names asked for, the programs that talked to it, and its hours. */
  destinationHistory(rangeSecs: number, addr: string): Promise<Data<DestinationHistoryView>>;
  /** The user's known programs, and programs that look like one without being it. */
  knownPrograms(): Promise<Data<KnownProgramsView>>;
  /** Asks the service to mark the product of the program at path as known. Never throws: a refusal is in the outcome. */
  markKnown(path: string, label: string, note?: string): Promise<WriteOutcome>;
  unmarkKnown(id: number): Promise<WriteOutcome>;
  /** What the service holds as owner data and whether a download is under way. Makes no network request. Never throws. */
  ownerDataStatus(): Promise<OwnerDataOutcome>;
  /** Asks the service to download the owner data (the only thing SYNTRAC ever downloads). Call only after the user agreed. Never throws. */
  refreshOwnerData(): Promise<OwnerDataOutcome>;
  /** The optional AI help: the settings (off by default). Never throws; the safe answer is "off". */
  aiSettings(): Promise<AiSettings>;
  /** Checks and keeps the settings; a refusal says why and keeps nothing. */
  saveAiSettings(settings: AiSettings): Promise<AiResult<AiSettings>>;
  /** The state of the model server (asks it only when the help is on and set up). */
  aiStatus(): Promise<AiStatus | null>;
  /** Starts the model server from the user's launch command. Only a button calls this. */
  startAiServer(): Promise<AiResult<null>>;
  /** Stops the model server this app started (never one that was already running). */
  stopAiServer(): Promise<AiResult<null>>;
  /** The prompt as it would be sent, or (external) as it would be copied for an outside assistant. Asks no server. */
  previewPrompt(subject: InvestigateSubject, external: boolean): Promise<AiResult<PromptPreview>>;
  /** Starts an investigation and returns its number; poll it for the answer. */
  startInvestigation(subject: InvestigateSubject): Promise<AiResult<number>>;
  pollInvestigation(id: number): Promise<Investigation | null>;
  cancelInvestigation(id: number): Promise<void>;
  /** The answer kept from an earlier investigation of this subject, or null. */
  savedAnswer(subject: InvestigateSubject): Promise<SavedAnswer | null>;
  /** Which subjects have a kept answer and since when (no answers), for marking lists. */
  savedAnswers(): Promise<SavedMark[]>;
  /** Throws the kept answer for a subject away. */
  forgetAnswer(subject: InvestigateSubject): Promise<void>;
  processes(params: ProcessListParams): Promise<Data<ProcessRowView[]>>;
  connections(rangeSecs: number, limit?: number): Promise<Data<ConnectionRowView[]>>;
}

function failure(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  return `The SYNTRAC backend could not be reached (${text}). This view only works inside the SYNTRAC app.`;
}

/** A failed call becomes an explicit state, so the UI never hangs on "Loading". */
async function data<T>(command: string, args?: Record<string, unknown>): Promise<Data<T>> {
  try {
    return await invoke<Data<T>>(command, args);
  } catch (error) {
    return { state: 'error', message: failure(error) };
  }
}

/** A failed write is an outcome with a message, so the screen can say what happened. */
async function write(command: string, args: Record<string, unknown>): Promise<WriteOutcome> {
  try {
    return await invoke<WriteOutcome>(command, args);
  } catch (error) {
    return { ok: false, id: null, message: failure(error) };
  }
}

async function ownerData(command: string): Promise<OwnerDataOutcome> {
  try {
    return await invoke<OwnerDataOutcome>(command);
  } catch (error) {
    return { ok: false, state: null, message: failure(error) };
  }
}

/** A command that returns Result<_, String> rejects with the message; that becomes an explicit result. */
async function attempt<T>(command: string, args?: Record<string, unknown>): Promise<AiResult<T>> {
  try {
    return { ok: true, value: await invoke<T>(command, args) };
  } catch (error) {
    return { ok: false, message: typeof error === 'string' ? error : failure(error) };
  }
}

const AI_OFF: AiSettings = {
  enabled: false,
  endpoint: '',
  model: '',
  detailLevel: 'summary',
  launchProgram: '',
  launchArgs: [],
  launchWorkingDir: '',
  startOnInvestigate: false,
  stopOnQuit: false,
};

export const tauriApi: Api = {
  async health() {
    try {
      return await invoke<HealthView>('get_health');
    } catch (error) {
      return {
        state: 'unknown',
        message: failure(error),
        version: null,
        pid: null,
        uptimeSecs: null,
        etwDropped: null,
        etwSilentSecs: null,
        dataAgeSecs: null,
        storage: null,
      };
    }
  },
  overview: (rangeSecs) => data<OverviewView>('get_overview', { rangeSecs }),
  systemHistory: (rangeSecs) => data<SystemHistoryView>('get_system_history', { rangeSecs }),
  topPrograms: (rangeSecs, metric, limit, groupByProduct) =>
    data<ProgramsView>('get_top_programs', { rangeSecs, metric, limit, groupByProduct }),
  programHistory: (rangeSecs, path) => data<ProgramHistoryView>('get_program_history', { rangeSecs, path }),
  processTree: (windowSecs) => data<ProcessTreeView>('get_process_tree', { windowSecs }),
  topDestinations: (rangeSecs, order, limit, allAddresses) =>
    data<DestinationsView>('get_top_destinations', { rangeSecs, order, limit, allAddresses }),
  topOwners: (rangeSecs, limit, allAddresses) => data<OwnersView>('get_top_owners', { rangeSecs, limit, allAddresses }),
  findings: (status) => data<FindingsView>('get_findings', { status, limit: undefined }),
  decideFinding: (id, decision, options) =>
    write('decide_finding', { id, decision, days: options?.days, note: options?.note, scope: options?.scope }),
  review: () => data<ReviewView>('get_review', { limit: undefined }),
  confirmReview: (root) => write('confirm_review', { root }),
  confirmReviewPrograms: (root, publisher) => write('confirm_review_programs', { root, publisher }),
  removeExpectation: (id) => write('remove_expectation', { id }),
  // On when the program cannot be asked: the default, and the safe side (a missed alert is worse than an extra one).
  notificationsOn: async () => {
    try {
      return await invoke<boolean>('get_notifications_on');
    } catch {
      return true;
    }
  },
  setNotificationsOn: async (on) => {
    try {
      await invoke('set_notifications_on', { on });
    } catch {
      // A switch that cannot be written stays as it was; the next read shows the truth.
    }
  },
  programDestinations: (rangeSecs, path, limit, allAddresses) =>
    data<ProgramDestinationsView>('get_program_destinations', { rangeSecs, path, limit, allAddresses }),
  newDestinations: (windowSecs, limit, allAddresses) =>
    data<NewDestinationsView>('get_new_destinations', { windowSecs, limit, allAddresses }),
  destinationHistory: (rangeSecs, addr) => data<DestinationHistoryView>('get_destination_history', { rangeSecs, addr }),
  knownPrograms: () => data<KnownProgramsView>('get_known_programs'),
  markKnown: (path, label, note) => write('mark_known_program', { path, label, note }),
  unmarkKnown: (id) => write('unmark_known_program', { id }),
  ownerDataStatus: () => ownerData('get_owner_data_status'),
  refreshOwnerData: () => ownerData('refresh_owner_data'),
  processes: (params) => data<ProcessRowView[]>('list_processes', { params }),
  connections: (rangeSecs, limit) => data<ConnectionRowView[]>('list_connections', { rangeSecs, limit }),
  aiSettings: async () => {
    try {
      return await invoke<AiSettings>('get_ai_settings');
    } catch {
      return AI_OFF;
    }
  },
  saveAiSettings: (settings) => attempt<AiSettings>('set_ai_settings', { settings }),
  aiStatus: async () => {
    try {
      return await invoke<AiStatus>('get_ai_status');
    } catch {
      return null;
    }
  },
  startAiServer: () => attempt<null>('start_ai_server'),
  stopAiServer: () => attempt<null>('stop_ai_server'),
  previewPrompt: (subject, external) => attempt<PromptPreview>('ai_preview_prompt', { subject, external }),
  startInvestigation: (subject) => attempt<number>('ai_start_investigation', { subject }),
  pollInvestigation: async (id) => {
    try {
      return await invoke<Investigation | null>('ai_poll_investigation', { id });
    } catch {
      return null;
    }
  },
  cancelInvestigation: async (id) => {
    try {
      await invoke('ai_cancel_investigation', { id });
    } catch {
      // Nothing to cancel if the program cannot be reached.
    }
  },
  savedAnswer: async (subject) => {
    try {
      return await invoke<SavedAnswer | null>('ai_saved_investigation', { subject });
    } catch {
      return null;
    }
  },
  savedAnswers: async () => {
    try {
      return await invoke<SavedMark[]>('ai_saved_list');
    } catch {
      return [];
    }
  },
  forgetAnswer: async (subject) => {
    try {
      await invoke('ai_forget_investigation', { subject });
    } catch {
      // Nothing to forget if the program cannot be reached.
    }
  },
};
