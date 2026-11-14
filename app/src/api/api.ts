import { invoke } from '@tauri-apps/api/core';
import type {
  ConnectionRowView,
  Data,
  HealthView,
  KnownProgramsView,
  OverviewView,
  ProcessListParams,
  ProcessRowView,
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
  /** The user's known programs, and programs that look like one without being it. */
  knownPrograms(): Promise<Data<KnownProgramsView>>;
  /** Asks the service to mark the product of the program at path as known. Never throws: a refusal is in the outcome. */
  markKnown(path: string, label: string, note?: string): Promise<WriteOutcome>;
  unmarkKnown(id: number): Promise<WriteOutcome>;
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
  knownPrograms: () => data<KnownProgramsView>('get_known_programs'),
  markKnown: (path, label, note) => write('mark_known_program', { path, label, note }),
  unmarkKnown: (id) => write('unmark_known_program', { id }),
  processes: (params) => data<ProcessRowView[]>('list_processes', { params }),
  connections: (rangeSecs, limit) => data<ConnectionRowView[]>('list_connections', { rangeSecs, limit }),
};
