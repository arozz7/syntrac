import { vi } from 'vitest';
import type { Api } from '../api/api';
import { fakeAiApi } from './fakeAi';
import type { Data, OwnerDataOutcome, WriteOutcome } from '../api/types';
import { findingsView, reviewView } from './fakeFindings';
import { NOW, connectionRow, health, overview, processRow, systemHistory } from './fakeHome';
import {
  destinationHistoryView,
  destinationsView,
  newDestinationsView,
  ownerDataState,
  ownersView,
  programDestinationsView,
} from './fakeNetwork';
import { knownProgramsView, programHistoryView, programsView } from './fakePrograms';

export * from './fakeHome';
export * from './fakePrograms';
export * from './fakeNetwork';
export * from './fakeFindings';

const ok = <T>(data: T): Data<T> => ({ state: 'ok', data });

/** An `Api` whose every method is a spy returning sensible sample data. */
export function makeFakeApi(overrides: Partial<Api> = {}): Api & { [K in keyof Api]: ReturnType<typeof vi.fn> } {
  return {
    health: vi.fn(async () => health()),
    overview: vi.fn(async () => ok(overview())),
    systemHistory: vi.fn(async () => ok(systemHistory())),
    ownerDataStatus: vi.fn(async (): Promise<OwnerDataOutcome> => ({
      ok: true,
      state: ownerDataState(),
      message: null,
    })),
    refreshOwnerData: vi.fn(async (): Promise<OwnerDataOutcome> => ({
      ok: true,
      state: ownerDataState({ refreshing: true }),
      message: null,
    })),
    topPrograms: vi.fn(async () => ok(programsView())),
    programHistory: vi.fn(async () => ok(programHistoryView())),
    topDestinations: vi.fn(async () => ok(destinationsView())),
    topOwners: vi.fn(async () => ok(ownersView())),
    findings: vi.fn(async () => ok(findingsView())),
    decideFinding: vi.fn(async (): Promise<WriteOutcome> => ({ ok: true, id: null, message: null })),
    removeExpectation: vi.fn(async (): Promise<WriteOutcome> => ({ ok: true, id: null, message: null })),
    ...fakeAiApi(),
    notificationsOn: vi.fn(async (): Promise<boolean> => true),
    setNotificationsOn: vi.fn(async (): Promise<void> => undefined),
    review: vi.fn(async () => ok(reviewView())),
    confirmReview: vi.fn(async (): Promise<WriteOutcome> => ({ ok: true, id: null, message: null })),
    confirmReviewPrograms: vi.fn(async (): Promise<WriteOutcome> => ({ ok: true, id: null, message: null })),
    programDestinations: vi.fn(async () => ok(programDestinationsView())),
    newDestinations: vi.fn(async () => ok(newDestinationsView())),
    destinationHistory: vi.fn(async () => ok(destinationHistoryView())),
    knownPrograms: vi.fn(async () => ok(knownProgramsView())),
    markKnown: vi.fn(async (): Promise<WriteOutcome> => ({ ok: true, id: 1, message: null })),
    unmarkKnown: vi.fn(async (): Promise<WriteOutcome> => ({ ok: true, id: null, message: null })),
    processes: vi.fn(async () =>
      ok([
        processRow(),
        processRow({ id: 'p2', pid: 11, name: 'gone.exe', running: false, endedMs: NOW - 1000 }),
        processRow({
          id: 'p3',
          pid: 12,
          name: 'lost.exe',
          running: false,
          endedMs: NOW - 2000,
          endedApproximate: true,
        }),
      ]),
    ),
    connections: vi.fn(async () =>
      ok([
        connectionRow(),
        connectionRow({ name: 'pid 99', pid: 99, processId: null, protocol: 'udp', remotePort: 5353 }),
      ]),
    ),
    ...overrides,
  } as Api & { [K in keyof Api]: ReturnType<typeof vi.fn> };
}
