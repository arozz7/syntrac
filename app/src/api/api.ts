import { invoke } from '@tauri-apps/api/core';
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
  ProgramDestinationsView,
  OverviewView,
  OwnerDataOutcome,
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
    options?: { days?: number; note?: string; scope?: 'this_item' | 'this_folder' },
  ): Promise<WriteOutcome>;
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
  topDestinations: (rangeSecs, order, limit, allAddresses) =>
    data<DestinationsView>('get_top_destinations', { rangeSecs, order, limit, allAddresses }),
  topOwners: (rangeSecs, limit, allAddresses) => data<OwnersView>('get_top_owners', { rangeSecs, limit, allAddresses }),
  findings: (status) => data<FindingsView>('get_findings', { status, limit: undefined }),
  decideFinding: (id, decision, options) =>
    write('decide_finding', { id, decision, days: options?.days, note: options?.note, scope: options?.scope }),
  removeExpectation: (id) => write('remove_expectation', { id }),
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
};
