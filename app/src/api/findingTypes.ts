// Views of findings, what the user decided about them, and the daily review (see types.ts).

export type FindingStatus = 'open' | 'expected' | 'not_expected' | 'snoozed' | 'resolved';

/** unusual: differs from what was seen before. suspicious: matches a pattern often seen in rogue software. confirmed: matches a list the user imported. */
export type FindingTier = 'unusual' | 'suspicious' | 'confirmed';

/** Something a detection rule found. Evidence, never a verdict. */
export interface FindingView {
  id: number;
  /** The rule that found it (new_program, ...). */
  kind: string;
  /** What it is about, as the stable key an expectation matches. */
  subject: string;
  subjectLabel: string;
  tier: FindingTier;
  title: string;
  /** One plain sentence: why it was raised. */
  why: string;
  evidence: string[];
  status: FindingStatus;
  firstAtMs: number;
  lastAtMs: number;
  hits: number;
  decidedAtMs: number | null;
  snoozedUntilMs: number | null;
  /** The install folder "always expected" could cover as a whole (a program in Windows or Program Files only); null when there is none to offer. */
  folderScope: string | null;
  /** A writable tool or project folder the user may deliberately call a build folder (new programs only); null where a program could simply be dropped in. */
  buildScope: string | null;
}

/** Something the user said is expected: one kind of finding about exactly one subject, or (older, or a build folder) a folder. */
export interface ExpectationView {
  id: number;
  kind: string;
  subject: string;
  note: string | null;
  createdMs: number;
  sourceFinding: number | null;
  ceiling: number | null;
  /** How wide it is: one subject, everything below a folder, or new programs below a build folder. */
  scope: 'item' | 'folder' | 'build';
  /** The folder of a `folder` or `build` expectation. */
  scopeRoot: string | null;
  /** A whole-folder expectation for a folder ordinary programs can write to: still honoured, but a program copied there is never called new. */
  writableWarning: boolean;
}

export interface FindingCountsView {
  open: number;
  notExpected: number;
  snoozed: number;
  expected: number;
  resolved: number;
}

export interface DetectionView {
  /** When detection was first switched on: "new" counts only what came after. */
  switchedOnMs: number;
  lastRunMs: number | null;
  /** What went wrong in the last run, in plain words. */
  lastError: string | null;
}

/** What appeared in the last day, counted and not raised. */
export interface DigestView {
  windowHours: number;
  newDestinations: number;
  newOwners: number;
  /** The owner data is on this machine; without it networks cannot be counted. */
  ownerData: boolean;
  /** Programs first seen in the window inside a build folder: not raised, so counted here and never invisible. */
  newInBuildFolders: number;
}

export interface FindingsView {
  digest: DigestView;
  /** Null: detection has never run (it is off, or has not had its first run yet). */
  detection: DetectionView | null;
  counts: FindingCountsView;
  findings: FindingView[];
  expectations: ExpectationView[];
}

export interface ReviewProgramView {
  path: string;
  name: string;
}

/** Programs of one product that were already here when detection was switched on. */
export interface ReviewGroupView {
  /** The install folder, lower case: what "mark as expected" covers. */
  root: string;
  label: string;
  publisher: string | null;
  count: number;
  /** The first few, by name. */
  programs: ReviewProgramView[];
  /** Why it deserves a closer look, as phrases; empty when it does not. */
  concerns: string[];
  location: string;
  /** The folder is protected, so the whole folder may be expected; otherwise only the programs, one by one. */
  wholeFolderAllowed: boolean;
}

export interface ReviewView {
  /** When detection was first switched on; null before that. */
  switchedOnMs: number | null;
  totalGroups: number;
  groups: ReviewGroupView[];
}

/** What the user can do with a finding. */
export type FindingDecision =
  'expected_this_time' | 'always_expected' | 'raise_limit' | 'not_expected' | 'snooze' | 'reopen';
