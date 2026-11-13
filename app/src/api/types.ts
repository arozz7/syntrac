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
