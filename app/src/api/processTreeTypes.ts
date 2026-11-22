// Views of the process tree: who started whom over a window (see types.ts).
import type { IdentityBadgeView } from './programTypes';

export interface ProcessNodeView {
  id: string;
  pid: number;
  name: string;
  imagePath: string;
  /** Cleaned and cut to a few hundred characters by the backend. */
  commandLine: string | null;
  startedMs: number;
  endedMs: number | null;
  /** The end time is only an upper bound (the process ended while the collector was not running). */
  endedApproximate: boolean;
  running: boolean;
  /** Its starter is named but was not alive in this window (or was not recorded). */
  parentMissing: boolean;
  /** Processes below this one that exist but are not shown. */
  hiddenDescendants: number;
  identity: IdentityBadgeView;
  children: ProcessNodeView[];
}

export interface ProcessTreeView {
  fromMs: number;
  toMs: number;
  /** Processes alive in the window, and how many of them are shown. */
  total: number;
  shown: number;
  roots: ProcessNodeView[];
}
