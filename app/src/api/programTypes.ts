// Views of programs: their identity, how they start, their standing on the known list and their history (see types.ts).

export type ProgramMetric = 'cpu' | 'memory' | 'starts' | 'network' | 'gpu';

/** Where a program runs from, as the backend classifies the path. */
export type LocationClass =
  'system' | 'program_files' | 'program_data' | 'user_profile' | 'temporary' | 'network' | 'other' | 'unknown';

/** What the service learned about a program's file. Evidence, never a verdict. */
export interface IdentityBadgeView {
  /** pending: not looked at yet. known: the file was read. unreadable: it could not be read, nothing is known. */
  state: 'pending' | 'known' | 'unreadable';
  signature: 'valid' | 'untrusted' | 'altered' | 'unsigned' | null;
  /** The signer, else the company in the version information. */
  publisher: string | null;
  product: string | null;
  location: LocationClass;
  /** The kinds of account it has been SEEN running as, most recent first. Empty: never observed running (not "none"). */
  runsAs: AccountKind[];
  /** It has been seen hosting a Windows service. */
  service: boolean;
  /** Something that starts with Windows was found for it and is still there. False means none was found in the places read. */
  startsAtBoot: boolean;
  /** The name an installer registered for the folder it lives in (the Uninstall keys), when a product still installed holds it. A clue. */
  registeredAs: string | null;
}

/** A product the Uninstall registry keys say is installed in the folder a program lives in. */
export interface RegisteredView {
  name: string;
  publisher: string | null;
  version: string | null;
  /** As the registry holds it (usually YYYYMMDD). */
  installed: string | null;
  /** The install folder, lower case. */
  folder: string;
  /** The folder was worked out from the icon or the uninstall command, not stated by the entry: weaker evidence. */
  folderDerived: boolean;
  /** The entry was still there when the keys were last read completely. */
  current: boolean;
  firstSeenMs: number;
  lastSeenMs: number;
}

/** One way a program gets started: a Windows service, a scheduled task, a Run key or a Startup folder entry. */
export type LauncherKind = 'service_auto' | 'service_other' | 'task' | 'run_key' | 'run_once' | 'startup_folder';

export interface LauncherView {
  kind: LauncherKind;
  /** Where it was found: the service name, the task path, the key and value name, the shortcut. */
  location: string;
  /** The triggers of a task, the start type of a service. */
  detail: string;
  arguments: string;
  firstSeenMs: number;
  lastSeenMs: number;
  /** It was still there when its kind was last read completely. */
  current: boolean;
}

/** What kind of account a program runs as, from its SID. Evidence of how it was started, not of whether it is safe. */
export type AccountKind = 'system' | 'local_service' | 'network_service' | 'service_account' | 'user' | 'other';

/** One account a program has been seen running as. */
export interface AccountView {
  /** As Windows names it (for example NT AUTHORITY\SYSTEM), when it could. */
  name: string | null;
  kind: AccountKind;
  firstSeenMs: number;
  lastSeenMs: number;
}

/** One Windows service a program has been seen hosting. */
export interface ServiceView {
  name: string;
  displayName: string | null;
  firstSeenMs: number;
  lastSeenMs: number;
}

export interface IdentityFailureView {
  kind: string;
  message: string;
  attempts: number;
}

export interface IdentityDetailView extends IdentityBadgeView {
  /** Who signed the file (the certificate's name), when it is signed. */
  signer: string | null;
  signatureKind: 'embedded' | 'catalog' | null;
  thumbprint: string | null;
  company: string | null;
  description: string | null;
  originalFilename: string | null;
  fileVersion: string | null;
  sha256: string | null;
  sizeBytes: number | null;
  modifiedMs: number | null;
  /** When the facts were measured. */
  inspectedAtMs: number | null;
  lastAttemptMs: number | null;
  /** The file carries the "downloaded from the Internet" mark. */
  downloaded: boolean;
  installRoot: string | null;
  /** The last look failed (any facts above are from an earlier look). */
  failure: IdentityFailureView | null;
  /** The accounts it has been seen running as, most recent first. Empty when it was never observed running. */
  accounts: AccountView[];
  /** The Windows services it has been seen hosting, most recent first. */
  services: ServiceView[];
  /** How it has been seen to get started, most recent first. Empty: nothing found (not "does not autostart"). */
  launchers: LauncherView[];
  /** The products the Uninstall keys register for the folder it lives in. Empty: none (not a claim). */
  registered: RegisteredView[];
}

/** Where a program stands against the user's list of known programs. Evidence, not a verdict. */
export interface StandingView {
  state: 'unlisted' | 'known' | 'lookalike';
  id: number | null;
  /** The known product's label (known and lookalike). */
  label: string | null;
  /** What the known product is signed by, lower case (lookalike, when it has a publisher). */
  expectedPublisher: string | null;
  /** The folder the known product lives in, lower case (lookalike). */
  expectedFolder: string | null;
  /** Why a lookalike looks like the known product, as sentences. */
  reasons: string[];
}

/** One program inside a product row. */
export interface ProgramMemberView {
  name: string;
  path: string;
  identity: IdentityBadgeView;
  standing: StandingView;
}

export interface ProgramRowView {
  name: string;
  /** Null for the bucket of processes whose program is unknown. */
  path: string | null;
  /** Averaged over every sample of the range: a program not among the heaviest processes counts as zero then. */
  cpuAvgPercent: number;
  cpuPeakPercent: number;
  memoryPeakBytes: number;
  /** The highest share of ONE GPU engine (not of the machine) and the highest dedicated GPU memory; 0 also means "no GPU data". */
  gpuPeakPercent: number;
  gpuMemoryPeakBytes: number;
  starts: number;
  /** In how many hours the program was recorded among the heaviest processes (out of `hoursInRange`). */
  hoursPresent: number;
  bytesSent: number;
  bytesReceived: number;
  destinations: number;
  firstSeenMs: number | null;
  lastSeenMs: number | null;
  identity: IdentityBadgeView;
  standing: StandingView;
  /** The programs of a product row, which has no path. Empty for an ordinary row. */
  members: ProgramMemberView[];
}

export interface ProgramsView {
  fromMs: number;
  toMs: number;
  historyStartsMs: number | null;
  /** Hours of the range that have data at all. */
  hoursInRange: number;
  programs: ProgramRowView[];
}

export interface ProgramHourView {
  hourMs: number;
  /** The samples behind the hour: the weight when hours are grouped into days. */
  samples: number;
  present: boolean;
  cpuAvgPercent: number;
  cpuPeakPercent: number;
  memoryPeakBytes: number;
  /** GPU as a share of ONE engine: the hour's average over all samples, its peak, and the peak of dedicated GPU memory. */
  gpuAvgPercent: number;
  gpuPeakPercent: number;
  gpuMemoryPeakBytes: number;
  starts: number;
}

export interface ProgramHistoryView {
  name: string;
  path: string;
  fromMs: number;
  toMs: number;
  firstSeenMs: number | null;
  lastSeenMs: number | null;
  identity: IdentityDetailView;
  standing: StandingView;
  hours: ProgramHourView[];
}

export interface KnownProductView {
  id: number;
  label: string;
  publisher: string | null;
  folder: string;
  productName: string | null;
  note: string | null;
  createdMs: number;
  /** File names of its programs seen on this machine. */
  programs: string[];
}

export interface LookalikeView {
  name: string;
  path: string;
  standing: StandingView;
  signature: IdentityBadgeView['signature'];
  publisher: string | null;
  location: LocationClass;
}

export interface KnownProgramsView {
  products: KnownProductView[];
  lookalikes: LookalikeView[];
}
