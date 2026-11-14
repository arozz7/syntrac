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
