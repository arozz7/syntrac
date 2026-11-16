import { vi } from 'vitest';
import type { Api } from '../api/api';
import type {
  ConnectionRowView,
  Data,
  DestinationHistoryView,
  DestinationRowView,
  DestinationsView,
  HealthView,
  OverviewView,
  ProcessCpuView,
  ProcessRowView,
  IdentityBadgeView,
  IdentityDetailView,
  KnownProgramsView,
  NewDestinationRowView,
  NewDestinationsView,
  OwnerDataOutcome,
  OwnerDataStateView,
  StandingView,
  WriteOutcome,
  ProgramHistoryView,
  ProgramRowView,
  ProgramsView,
  SystemHistoryView,
  SystemHourView,
} from '../api/types';

export const NOW = new Date(2026, 10, 12, 9, 5, 7).getTime();

export function health(overrides: Partial<HealthView> = {}): HealthView {
  return {
    state: 'running',
    message: 'The collector is running.',
    version: '0.1.0',
    pid: 4242,
    uptimeSecs: 3600,
    etwDropped: 0,
    etwSilentSecs: 0,
    dataAgeSecs: 3,
    storage: null,
    ...overrides,
  };
}

export function cpuProcess(name: string, avg: number, pid = 100): ProcessCpuView {
  return {
    instance: `id-${pid}`,
    pid,
    name,
    imagePath: `C:\\apps\\${name}`,
    avgPercent: avg,
    peakPercent: avg * 2,
    samplesPresent: 10,
  };
}

export function overview(overrides: Partial<OverviewView> = {}): OverviewView {
  return {
    fromMs: NOW - 900_000,
    toMs: NOW,
    system: [
      {
        tsMs: NOW - 10_000,
        cpuPercent: 12,
        memoryPercent: 52,
        netReceivedBytes: 1024,
        netSentBytes: 512,
        diskReadBytes: 0,
        diskWriteBytes: 2048,
      },
      {
        tsMs: NOW,
        cpuPercent: 18.5,
        memoryPercent: 53,
        netReceivedBytes: 2048,
        netSentBytes: 1024,
        diskReadBytes: 0,
        diskWriteBytes: 0,
      },
    ],
    cpu: {
      windowSamples: 174,
      systemAvgPercent: 12,
      othersAvgPercent: 0.4,
      unaccountedAvgPercent: 4.9,
      processes: [cpuProcess('chrome.exe', 3.2, 1), cpuProcess('MsMpEng.exe', 1.4, 2)],
    },
    destinations: [
      {
        remoteAddr: '57.144.180.192',
        remotePort: 443,
        bytesSent: 158_970,
        bytesReceived: 23_342_985,
        flowCount: 13,
        processCount: 1,
      },
    ],
    ...overrides,
  };
}

export function processRow(overrides: Partial<ProcessRowView> = {}): ProcessRowView {
  return {
    id: 'p1',
    pid: 10,
    parentId: null,
    name: 'worker.exe',
    imagePath: 'C:\\apps\\worker.exe',
    commandLine: null,
    startedMs: NOW - 60_000,
    endedMs: null,
    endedApproximate: false,
    running: true,
    ...overrides,
  };
}

export function connectionRow(overrides: Partial<ConnectionRowView> = {}): ConnectionRowView {
  return {
    startMs: NOW - 30_000,
    endMs: NOW - 20_000,
    processId: 'p1',
    pid: 10,
    name: 'browser.exe',
    protocol: 'tcp',
    remoteAddr: '203.0.113.9',
    remotePort: 443,
    bytesSent: 2048,
    bytesReceived: 4_194_304,
    opened: 1,
    closed: 0,
    failed: 0,
    ...overrides,
  };
}

export function systemHour(hourMs: number, overrides: Partial<SystemHourView> = {}): SystemHourView {
  return {
    hourMs,
    samples: 120,
    cpuAvgPercent: 12,
    cpuMaxPercent: 35,
    memoryAvgPercent: 52,
    memoryMaxPercent: 58,
    netReceivedBytes: 4096,
    netSentBytes: 1024,
    diskReadBytes: 2048,
    diskWriteBytes: 8192,
    othersCpuAvgPercent: 1,
    ...overrides,
  };
}

export function systemHistory(overrides: Partial<SystemHistoryView> = {}): SystemHistoryView {
  return {
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    historyStartsMs: NOW - 5 * 86_400_000,
    hours: [systemHour(NOW - 3 * 3_600_000), systemHour(NOW - 2 * 3_600_000, { cpuAvgPercent: 20 })],
    ...overrides,
  };
}

export function identityBadge(overrides: Partial<IdentityBadgeView> = {}): IdentityBadgeView {
  return {
    state: 'known',
    signature: 'valid',
    publisher: 'Google LLC',
    product: 'Google Chrome',
    location: 'program_files',
    runsAs: [],
    service: false,
    startsAtBoot: false,
    ...overrides,
  };
}

export function identityDetail(overrides: Partial<IdentityDetailView> = {}): IdentityDetailView {
  return {
    ...identityBadge(),
    signer: 'Google LLC',
    signatureKind: 'embedded',
    thumbprint: 'AB12CD34',
    company: 'Google LLC',
    description: 'Google Chrome',
    originalFilename: 'chrome.exe',
    fileVersion: '126.0.1',
    sha256: 'ab'.repeat(32),
    sizeBytes: 1024 ** 2,
    modifiedMs: NOW - 10 * 86_400_000,
    inspectedAtMs: NOW - 86_400_000,
    lastAttemptMs: NOW - 86_400_000,
    downloaded: false,
    installRoot: 'c:\\program files\\google',
    failure: null,
    accounts: [],
    services: [],
    launchers: [],
    ...overrides,
  };
}

export function standing(overrides: Partial<StandingView> = {}): StandingView {
  return {
    state: 'unlisted',
    id: null,
    label: null,
    expectedPublisher: null,
    expectedFolder: null,
    reasons: [],
    ...overrides,
  };
}

export function knownProgramsView(overrides: Partial<KnownProgramsView> = {}): KnownProgramsView {
  return { products: [], lookalikes: [], ...overrides };
}

export function programRow(overrides: Partial<ProgramRowView> = {}): ProgramRowView {
  return {
    name: 'chrome.exe',
    path: 'C:\\apps\\chrome.exe',
    cpuAvgPercent: 10,
    cpuPeakPercent: 20,
    memoryPeakBytes: 1024 ** 3,
    starts: 7,
    hoursPresent: 1,
    bytesSent: 1024,
    bytesReceived: 4096,
    destinations: 3,
    firstSeenMs: NOW - 86_400_000,
    lastSeenMs: NOW - 3_600_000,
    identity: identityBadge(),
    standing: standing(),
    members: [],
    ...overrides,
  };
}

export function programsView(overrides: Partial<ProgramsView> = {}): ProgramsView {
  return {
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    historyStartsMs: NOW - 86_400_000,
    hoursInRange: 2,
    programs: [
      programRow(),
      programRow({ name: 'code.exe', path: 'C:\\apps\\code.exe', cpuAvgPercent: 1.3, cpuPeakPercent: 4, starts: 2 }),
    ],
    ...overrides,
  };
}

export function programHistoryView(overrides: Partial<ProgramHistoryView> = {}): ProgramHistoryView {
  return {
    name: 'chrome.exe',
    path: 'C:\\apps\\chrome.exe',
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    firstSeenMs: NOW - 86_400_000,
    lastSeenMs: NOW - 3_600_000,
    identity: identityDetail(),
    standing: standing(),
    hours: [
      {
        hourMs: NOW - 3 * 3_600_000,
        samples: 120,
        present: true,
        cpuAvgPercent: 10,
        cpuPeakPercent: 20,
        memoryPeakBytes: 1024 ** 3,
        starts: 2,
      },
      {
        hourMs: NOW - 2 * 3_600_000,
        samples: 120,
        present: false,
        cpuAvgPercent: 0,
        cpuPeakPercent: 0,
        memoryPeakBytes: 0,
        starts: 0,
      },
    ],
    ...overrides,
  };
}

export function ownerDataState(overrides: Partial<OwnerDataStateView> = {}): OwnerDataStateView {
  return {
    dataset: { source: 'iptoasn.com', downloadedMs: NOW - 3 * 86_400_000, rows: 480_000 },
    refreshing: false,
    lastError: null,
    allowed: true,
    ...overrides,
  };
}

export function destinationRow(overrides: Partial<DestinationRowView> = {}): DestinationRowView {
  return {
    addr: '142.251.40.238',
    name: 'calendar.google.com',
    otherNames: 2,
    class: 'public',
    owner: null,
    bytesSent: 2048,
    bytesReceived: 5 * 1024 ** 2,
    flows: 40,
    programs: 2,
    topPrograms: [
      { name: 'chrome.exe', path: 'C:\\apps\\chrome.exe' },
      { name: 'Teams.exe', path: 'C:\\apps\\Teams.exe' },
    ],
    morePrograms: 0,
    firstSeenMs: NOW - 5 * 86_400_000,
    lastSeenMs: NOW - 3_600_000,
    ...overrides,
  };
}

export function destinationsView(overrides: Partial<DestinationsView> = {}): DestinationsView {
  return {
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    historyStartsMs: NOW - 5 * 86_400_000,
    hoursInRange: 100,
    ownerData: null,
    destinations: [
      destinationRow(),
      destinationRow({
        addr: '203.0.113.50',
        name: null,
        otherNames: 0,
        bytesReceived: 1024,
        flows: 3,
        programs: 1,
        topPrograms: [{ name: 'Unknown program', path: null }],
      }),
    ],
    ...overrides,
  };
}

export function newDestinationRow(overrides: Partial<NewDestinationRowView> = {}): NewDestinationRowView {
  return {
    addr: '198.51.100.7',
    name: 'later.example.org',
    otherNames: 0,
    class: 'public',
    owner: null,
    firstSeenMs: NOW - 3_600_000,
    lastSeenMs: NOW - 1_800_000,
    firstProgram: 'code.exe',
    firstProgramPath: 'C:\\apps\\code.exe',
    ...overrides,
  };
}

export function newDestinationsView(overrides: Partial<NewDestinationsView> = {}): NewDestinationsView {
  return {
    sinceMs: NOW - 4 * 86_400_000,
    baselineUntilMs: NOW - 4 * 86_400_000,
    historyStartsMs: NOW - 5 * 86_400_000,
    destinations: [newDestinationRow()],
    ...overrides,
  };
}

export function destinationHistoryView(overrides: Partial<DestinationHistoryView> = {}): DestinationHistoryView {
  return {
    addr: '142.251.40.238',
    class: 'public',
    owner: null,
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    firstSeenMs: NOW - 5 * 86_400_000,
    lastSeenMs: NOW - 3_600_000,
    names: [
      { name: 'calendar.google.com', firstSeenMs: NOW - 4 * 86_400_000, lastSeenMs: NOW - 3_600_000 },
      { name: 'clients4.google.com', firstSeenMs: NOW - 5 * 86_400_000, lastSeenMs: NOW - 2 * 86_400_000 },
    ],
    programs: [
      {
        name: 'chrome.exe',
        path: 'C:\\apps\\chrome.exe',
        bytesSent: 1024,
        bytesReceived: 4096,
        flows: 30,
        firstSeenMs: NOW - 5 * 86_400_000,
        lastSeenMs: NOW - 3_600_000,
      },
    ],
    hours: [
      { hourMs: NOW - 3 * 3_600_000, bytesSent: 1024, bytesReceived: 4096, flows: 30 },
      { hourMs: NOW - 2 * 3_600_000, bytesSent: 0, bytesReceived: 0, flows: 0 },
    ],
    ...overrides,
  };
}

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
