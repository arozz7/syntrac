// Sample data for the process tree (see fakeApi.ts).

import type { ProcessNodeView, ProcessTreeView } from '../api/types';
import { NOW } from './fakeHome';
import { identityBadge } from './fakePrograms';

export function treeNode(overrides: Partial<ProcessNodeView> = {}): ProcessNodeView {
  return {
    id: 'n1',
    pid: 100,
    name: 'explorer.exe',
    imagePath: String.raw`C:\Windows\explorer.exe`,
    commandLine: null,
    startedMs: NOW - 3_600_000,
    endedMs: null,
    endedApproximate: false,
    running: true,
    parentMissing: false,
    hiddenDescendants: 0,
    identity: identityBadge(),
    children: [],
    ...overrides,
  };
}

/** explorer.exe started editor.exe, which started (and has finished) compiler.exe; a launcher's child lost its parent. */
export function processTreeView(overrides: Partial<ProcessTreeView> = {}): ProcessTreeView {
  const compiler = treeNode({
    id: 'n3',
    pid: 102,
    name: 'compiler.exe',
    imagePath: String.raw`C:\tools\compiler.exe`,
    commandLine: 'compiler.exe --release',
    startedMs: NOW - 1_800_000,
    endedMs: NOW - 1_700_000,
    running: false,
  });
  const editor = treeNode({
    id: 'n2',
    pid: 101,
    name: 'editor.exe',
    imagePath: String.raw`C:\apps\editor.exe`,
    startedMs: NOW - 3_000_000,
    children: [compiler],
  });
  const explorer = treeNode({ children: [editor] });
  const orphan = treeNode({
    id: 'n4',
    pid: 200,
    name: 'worker.exe',
    imagePath: String.raw`C:\apps\worker.exe`,
    parentMissing: true,
    startedMs: NOW - 600_000,
  });
  return {
    fromMs: NOW - 86_400_000,
    toMs: NOW,
    total: 4,
    shown: 4,
    read: 4,
    truncated: false,
    roots: [explorer, orphan],
    ...overrides,
  };
}
