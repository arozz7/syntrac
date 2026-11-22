// Sample data for programs: identity, standing and history (see fakeApi.ts).

import type {
  IdentityBadgeView,
  IdentityDetailView,
  KnownProgramsView,
  ProgramHistoryView,
  ProgramRowView,
  ProgramsView,
  StandingView,
} from '../api/types';
import { NOW } from './fakeHome';

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
    registeredAs: null,
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
    registered: [],
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
    gpuPeakPercent: 0,
    gpuMemoryPeakBytes: 0,
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
        gpuAvgPercent: 0,
        gpuPeakPercent: 0,
        gpuMemoryPeakBytes: 0,
        starts: 2,
      },
      {
        hourMs: NOW - 2 * 3_600_000,
        samples: 120,
        present: false,
        cpuAvgPercent: 0,
        cpuPeakPercent: 0,
        memoryPeakBytes: 0,
        gpuAvgPercent: 0,
        gpuPeakPercent: 0,
        gpuMemoryPeakBytes: 0,
        starts: 0,
      },
    ],
    ...overrides,
  };
}
