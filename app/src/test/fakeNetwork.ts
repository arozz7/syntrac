// Sample data for destinations, owners and what programs connected to (see fakeApi.ts).

import type {
  DestinationHistoryView,
  DestinationRowView,
  DestinationsView,
  NewDestinationRowView,
  NewDestinationsView,
  OwnerDataStateView,
  OwnerRowView,
  OwnersView,
  ProgramDestinationRowView,
  ProgramDestinationsView,
} from '../api/types';
import { NOW } from './fakeHome';

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

export function programDestinationRow(overrides: Partial<ProgramDestinationRowView> = {}): ProgramDestinationRowView {
  return {
    addr: '142.251.40.238',
    name: 'calendar.google.com',
    otherNames: 0,
    class: 'public',
    owner: { asn: 15169, org: 'GOOGLE', country: 'US' },
    bytesSent: 2048,
    bytesReceived: 5 * 1024 ** 2,
    flows: 40,
    firstSeenMs: NOW - 5 * 86_400_000,
    lastSeenMs: NOW - 3_600_000,
    isNew: false,
    ...overrides,
  };
}

export function programDestinationsView(overrides: Partial<ProgramDestinationsView> = {}): ProgramDestinationsView {
  return {
    path: 'C:\\apps\\chrome.exe',
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    totalDestinations: 2,
    ownerData: { source: 'iptoasn.com', downloadedMs: NOW - 3 * 86_400_000, rows: 480_000 },
    destinations: [
      programDestinationRow(),
      programDestinationRow({
        addr: '203.0.113.50',
        name: null,
        owner: null,
        bytesSent: 10,
        bytesReceived: 1024,
        flows: 2,
        firstSeenMs: NOW - 86_400_000,
        isNew: true,
      }),
    ],
    ...overrides,
  };
}

export function ownerRow(overrides: Partial<OwnerRowView> = {}): OwnerRowView {
  return {
    owner: { asn: 15169, org: 'GOOGLE', country: 'US' },
    bytesSent: 4096,
    bytesReceived: 8 * 1024 ** 2,
    flows: 60,
    destinations: 7,
    topDestinations: [
      { addr: '142.251.40.238', name: 'calendar.google.com', bytesSent: 2048, bytesReceived: 5 * 1024 ** 2 },
      { addr: '142.250.1.1', name: null, bytesSent: 2048, bytesReceived: 3 * 1024 ** 2 },
    ],
    moreDestinations: 5,
    ...overrides,
  };
}

export function ownersView(overrides: Partial<OwnersView> = {}): OwnersView {
  return {
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    historyStartsMs: NOW - 5 * 86_400_000,
    hoursInRange: 100,
    ownerData: { source: 'iptoasn.com', downloadedMs: NOW - 3 * 86_400_000, rows: 480_000 },
    owners: [
      ownerRow(),
      ownerRow({
        owner: { asn: 16509, org: 'AMAZON-02', country: 'US' },
        bytesSent: 100,
        bytesReceived: 2 * 1024 ** 2,
        flows: 9,
        destinations: 1,
        topDestinations: [{ addr: '52.1.2.3', name: null, bytesSent: 100, bytesReceived: 2 * 1024 ** 2 }],
        moreDestinations: 0,
      }),
      ownerRow({
        owner: null,
        bytesSent: 10,
        bytesReceived: 1024,
        flows: 3,
        destinations: 2,
        topDestinations: [{ addr: '203.0.113.50', name: null, bytesSent: 10, bytesReceived: 1024 }],
        moreDestinations: 1,
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
