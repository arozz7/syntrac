// Sample data for findings, expectations and the daily review (see fakeApi.ts).

import type { ExpectationView, FindingView, FindingsView, ReviewGroupView, ReviewView } from '../api/types';
import { NOW } from './fakeHome';

export function findingRow(overrides: Partial<FindingView> = {}): FindingView {
  return {
    id: 1,
    kind: 'new_program',
    subject: 'c:\\users\\someone\\downloads\\tool.exe',
    subjectLabel: 'tool.exe',
    tier: 'suspicious',
    title: 'New program tool.exe',
    why: 'It was first seen after detection was switched on, and it is not signed.',
    evidence: ['Folder: C:\\Users\\someone\\Downloads', 'Signature: not signed'],
    status: 'open',
    firstAtMs: NOW - 2 * 86_400_000,
    lastAtMs: NOW - 3_600_000,
    hits: 3,
    decidedAtMs: null,
    snoozedUntilMs: null,
    folderScope: null,
    buildScope: null,
    ...overrides,
  };
}

export function expectationRow(overrides: Partial<ExpectationView> = {}): ExpectationView {
  return {
    id: 5,
    kind: 'new_program',
    subject: 'c:\\program files\\acme\\acmeserv.exe',
    note: 'my backup',
    createdMs: NOW - 86_400_000,
    sourceFinding: 2,
    ceiling: null,
    scope: 'item',
    scopeRoot: null,
    writableWarning: false,
    ...overrides,
  };
}

export function reviewGroup(overrides: Partial<ReviewGroupView> = {}): ReviewGroupView {
  return {
    root: 'c:\\program files\\acme',
    label: 'Acme Suite',
    publisher: 'acme inc',
    count: 3,
    programs: [
      { path: 'C:\\Program Files\\Acme\\a.exe', name: 'a.exe' },
      { path: 'C:\\Program Files\\Acme\\b.exe', name: 'b.exe' },
    ],
    concerns: [],
    location: 'Program Files',
    wholeFolderAllowed: true,
    ...overrides,
  };
}

export function reviewView(overrides: Partial<ReviewView> = {}): ReviewView {
  return {
    switchedOnMs: NOW - 5 * 86_400_000,
    totalGroups: 2,
    groups: [
      reviewGroup({
        root: 'c:\\users\\someone\\downloads',
        label: 'downloads',
        publisher: null,
        count: 1,
        programs: [{ path: 'C:\\Users\\someone\\Downloads\\tool.exe', name: 'tool.exe' }],
        concerns: ['it is not signed', 'it runs from a user folder'],
        location: 'a user folder',
        wholeFolderAllowed: false,
      }),
      reviewGroup(),
    ],
    ...overrides,
  };
}

export function findingsView(overrides: Partial<FindingsView> = {}): FindingsView {
  return {
    digest: { windowHours: 24, newDestinations: 0, newOwners: 0, ownerData: true, newInBuildFolders: 0 },
    detection: { switchedOnMs: NOW - 5 * 86_400_000, lastRunMs: NOW - 120_000, lastError: null },
    counts: { open: 2, notExpected: 1, snoozed: 0, expected: 3, resolved: 0 },
    findings: [
      findingRow(),
      findingRow({
        id: 2,
        tier: 'unusual',
        title: 'New program other.exe',
        subjectLabel: 'other.exe',
        subject: 'other',
        hits: 1,
      }),
    ],
    expectations: [expectationRow()],
    ...overrides,
  };
}
