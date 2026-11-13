import { invoke } from '@tauri-apps/api/core';
import type { ConnectionRowView, Data, HealthView, OverviewView, ProcessListParams, ProcessRowView } from './types';

/** Everything the UI can ask of the backend. Components depend on this interface, never on Tauri directly. */
export interface Api {
  health(): Promise<HealthView>;
  overview(rangeSecs: number): Promise<Data<OverviewView>>;
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
        dataAgeSecs: null,
      };
    }
  },
  overview: (rangeSecs) => data<OverviewView>('get_overview', { rangeSecs }),
  processes: (params) => data<ProcessRowView[]>('list_processes', { params }),
  connections: (rangeSecs, limit) => data<ConnectionRowView[]>('list_connections', { rangeSecs, limit }),
};
