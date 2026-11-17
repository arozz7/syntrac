// Mirrors the view models in app/src-tauri/src/views.rs and health.rs (camelCase on the wire).
// Times are milliseconds since the Unix epoch.

/** What a data call produced. `state` decides which screen to show. */
export type Data<T> =
  | { state: 'ok'; data: T }
  | { state: 'no_database'; message: string }
  | { state: 'schema_mismatch'; message: string }
  | { state: 'error'; message: string };

export type HealthState = 'running' | 'stale' | 'degraded' | 'offline' | 'unknown';

export interface HealthView {
  state: HealthState;
  message: string;
  version: string | null;
  pid: number | null;
  uptimeSecs: number | null;
  etwDropped: number | null;
  /** Seconds since ETW last delivered an event (null when ETW is not running). */
  etwSilentSecs: number | null;
  dataAgeSecs: number | null;
  /** The database and its drive, as the collector last measured them (null when it does not say). */
  storage: StorageView | null;
}

export interface SystemPointView {
  tsMs: number;
  cpuPercent: number;
  memoryPercent: number;
  netReceivedBytes: number;
  netSentBytes: number;
  diskReadBytes: number;
  diskWriteBytes: number;
}

export interface ProcessCpuView {
  instance: string | null;
  pid: number;
  name: string;
  imagePath: string | null;
  avgPercent: number;
  peakPercent: number;
  samplesPresent: number;
}

export interface CpuView {
  windowSamples: number;
  systemAvgPercent: number;
  othersAvgPercent: number;
  /** System CPU that no measured process explains (interrupt/driver time, unreadable processes). */
  unaccountedAvgPercent: number;
  processes: ProcessCpuView[];
}

export interface DestinationView {
  remoteAddr: string;
  remotePort: number;
  bytesSent: number;
  bytesReceived: number;
  flowCount: number;
  processCount: number;
}

export interface OverviewView {
  fromMs: number;
  toMs: number;
  system: SystemPointView[];
  cpu: CpuView;
  destinations: DestinationView[];
}

export interface ProcessListParams {
  runningOnly: boolean;
  search?: string;
  limit?: number;
}

export interface ProcessRowView {
  id: string;
  pid: number;
  parentId: string | null;
  name: string;
  imagePath: string;
  commandLine: string | null;
  startedMs: number;
  endedMs: number | null;
  /** The exit time is only an upper bound: the process exited while the collector was not running. */
  endedApproximate: boolean;
  running: boolean;
}

export interface ConnectionRowView {
  startMs: number;
  endMs: number;
  processId: string | null;
  pid: number;
  name: string;
  protocol: 'tcp' | 'udp';
  remoteAddr: string;
  remotePort: number;
  bytesSent: number;
  bytesReceived: number;
  opened: number;
  closed: number;
  failed: number;
}

/** One completed hour (UTC) of the machine as a whole, from the hourly rollups. */
export interface SystemHourView {
  hourMs: number;
  /** The samples the hour is built from; used to weight averages when hours are grouped into days. */
  samples: number;
  cpuAvgPercent: number;
  cpuMaxPercent: number;
  memoryAvgPercent: number;
  memoryMaxPercent: number;
  netReceivedBytes: number;
  netSentBytes: number;
  diskReadBytes: number;
  diskWriteBytes: number;
  othersCpuAvgPercent: number;
}

export interface SystemHistoryView {
  fromMs: number;
  toMs: number;
  /** Where the history begins (the oldest rollup), whatever range was asked for. */
  historyStartsMs: number | null;
  /** Completed hours only, oldest first. A missing hour is a gap, never a zero. */
  hours: SystemHourView[];
}

export type ProgramMetric = 'cpu' | 'memory' | 'starts' | 'network';

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

/** The answer to a change request: the service's own words when it refuses. */
export interface WriteOutcome {
  ok: boolean;
  id: number | null;
  message: string | null;
}

export type StorageLevel = 'ok' | 'trimming' | 'paused';

export interface StorageView {
  level: StorageLevel;
  /** Bytes of live data in the database file. */
  databaseLiveBytes: number;
  databaseFileBytes: number;
  driveFreeBytes: number;
  /** Milliseconds since the Unix epoch; null when there is none yet. */
  oldestDetailMs: number | null;
  oldestRollupMs: number | null;
  /** Hours of detail the storage guard has trimmed early since the service started. */
  trimmedHoursTotal: number;
}

/** What kind of address a destination is: the internet, or this network's own (router, printers), or multicast. */
export type AddressClass = 'public' | 'private' | 'link_local' | 'multicast' | 'other';

export type DestinationOrder = 'bytes' | 'flows' | 'programs';

/** Who owns the network range an address is in (from the downloaded owner data), and the country it is registered in. */
export interface OwnerView {
  asn: number;
  org: string;
  country: string;
}

/** The owner data on this machine: where and when it was downloaded, and how many network ranges it holds. */
export interface OwnerDataView {
  source: string;
  downloadedMs: number;
  rows: number;
}

/** What the service says about the owner data: what it holds and whether a download is under way. */
export interface OwnerDataStateView {
  dataset: OwnerDataView | null;
  refreshing: boolean;
  /** Why the last download failed, in plain words. */
  lastError: string | null;
  /** False when the service is configured never to download. */
  allowed: boolean;
}

/** The answer to asking the service about owner data: its state, or why it could not say. */
export interface OwnerDataOutcome {
  ok: boolean;
  state: OwnerDataStateView | null;
  message: string | null;
}

export interface DestinationRowView {
  addr: string;
  /** The name a program asked for most recently when it reached this address (never an owner). */
  name: string | null;
  otherNames: number;
  class: AddressClass;
  /** Who owns the network range it is in; null without downloaded owner data or outside every range. */
  owner: OwnerView | null;
  bytesSent: number;
  bytesReceived: number;
  flows: number;
  /** How many different programs talked to it. */
  programs: number;
  /** The biggest of those programs by traffic, so a row says who without a click. */
  topPrograms: RowProgramView[];
  /** How many programs are not in `topPrograms`. */
  morePrograms: number;
  firstSeenMs: number | null;
  lastSeenMs: number | null;
}

/** A program named in a row. `path` is null for the bucket of programs that could not be identified. */
export interface RowProgramView {
  name: string;
  path: string | null;
}

export interface DestinationsView {
  fromMs: number;
  toMs: number;
  historyStartsMs: number | null;
  /** Hours of the range that have data at all. */
  hoursInRange: number;
  /** The owner data, when it has been downloaded. */
  ownerData: OwnerDataView | null;
  destinations: DestinationRowView[];
}

/** One of an owner's destinations, to say where its traffic went without opening the owner. */
export interface OwnerDestinationView {
  addr: string;
  name: string | null;
  bytesSent: number;
  bytesReceived: number;
}

/** One network owner over a range. `owner` is null for the group of destinations with no owner found. */
export interface OwnerRowView {
  owner: OwnerView | null;
  bytesSent: number;
  bytesReceived: number;
  flows: number;
  destinations: number;
  /** The biggest destinations by traffic. */
  topDestinations: OwnerDestinationView[];
  /** How many destinations are not in `topDestinations`. */
  moreDestinations: number;
}

export interface OwnersView {
  fromMs: number;
  toMs: number;
  historyStartsMs: number | null;
  hoursInRange: number;
  /** The owner data, when it has been downloaded; without it every destination is in the group with no owner. */
  ownerData: OwnerDataView | null;
  owners: OwnerRowView[];
}

/** One address a program talked to over a range. */
export interface ProgramDestinationRowView {
  addr: string;
  name: string | null;
  otherNames: number;
  class: AddressClass;
  owner: OwnerView | null;
  bytesSent: number;
  bytesReceived: number;
  flows: number;
  /** When THIS program first talked to the address (permanent). */
  firstSeenMs: number | null;
  /** The last time within the range. */
  lastSeenMs: number;
  /** The program first reached it after the first day of history and within the range. */
  isNew: boolean;
}

export interface ProgramDestinationsView {
  path: string;
  fromMs: number;
  toMs: number;
  /** How many different addresses the program talked to in the range, whatever the limit. */
  totalDestinations: number;
  ownerData: OwnerDataView | null;
  /** The biggest by traffic, biggest first. */
  destinations: ProgramDestinationRowView[];
}

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
}

/** Something the user said is expected: one kind of finding about exactly one subject. */
export interface ExpectationView {
  id: number;
  kind: string;
  subject: string;
  note: string | null;
  createdMs: number;
  sourceFinding: number | null;
  ceiling: number | null;
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

export interface FindingsView {
  /** Null: detection has never run (it is off, or has not had its first run yet). */
  detection: DetectionView | null;
  counts: FindingCountsView;
  findings: FindingView[];
  expectations: ExpectationView[];
}

/** What the user can do with a finding. */
export type FindingDecision = 'expected_this_time' | 'always_expected' | 'not_expected' | 'snooze' | 'reopen';

export interface NewDestinationRowView {
  addr: string;
  name: string | null;
  otherNames: number;
  class: AddressClass;
  owner: OwnerView | null;
  firstSeenMs: number;
  lastSeenMs: number;
  /** The program that was first to talk to it; null when only unidentified programs ever did. */
  firstProgram: string | null;
  firstProgramPath: string | null;
}

export interface NewDestinationsView {
  /** Only addresses first seen at or after this count as new (null: there is no history yet). */
  sinceMs: number | null;
  /** Everything first seen before this is the baseline, not news. */
  baselineUntilMs: number | null;
  historyStartsMs: number | null;
  destinations: NewDestinationRowView[];
}

export interface DestinationNameView {
  name: string;
  firstSeenMs: number;
  lastSeenMs: number;
}

export interface DestinationProgramView {
  name: string;
  /** Null for the bucket of programs that could not be identified. */
  path: string | null;
  bytesSent: number;
  bytesReceived: number;
  flows: number;
  firstSeenMs: number;
  lastSeenMs: number;
}

export interface DestinationHourView {
  hourMs: number;
  bytesSent: number;
  bytesReceived: number;
  flows: number;
}

export interface DestinationHistoryView {
  addr: string;
  class: AddressClass;
  owner: OwnerView | null;
  fromMs: number;
  toMs: number;
  firstSeenMs: number | null;
  lastSeenMs: number | null;
  /** Every name asked for when this address was reached, newest first. */
  names: DestinationNameView[];
  programs: DestinationProgramView[];
  hours: DestinationHourView[];
}
