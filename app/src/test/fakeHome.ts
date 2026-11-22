// Sample data for the overview, processes, connections and system history (see fakeApi.ts).

import type {
  ConnectionRowView,
  HealthView,
  OverviewView,
  ProcessCpuView,
  ProcessGpuView,
  ProcessRowView,
  SystemHistoryView,
  SystemHourView,
} from '../api/types';

export const NOW = new Date(2026, 10, 12, 9, 5, 7).getTime();

export function health(overrides: Partial<HealthView> = {}): HealthView {
  return {
    state: 'running',
    message: 'The collector is running.',
    version: '0.1.0',
    pid: 4242,
    uptimeSecs: 3600,
    etwDropped: 0,
    etwSilentSecs: 0,
    dataAgeSecs: 3,
    storage: null,
    ...overrides,
  };
}

export function cpuProcess(name: string, avg: number, pid = 100): ProcessCpuView {
  return {
    instance: `id-${pid}`,
    pid,
    name,
    imagePath: `C:\\apps\\${name}`,
    avgPercent: avg,
    peakPercent: avg * 2,
    samplesPresent: 10,
  };
}

export function overview(overrides: Partial<OverviewView> = {}): OverviewView {
  return {
    fromMs: NOW - 900_000,
    toMs: NOW,
    system: [
      {
        tsMs: NOW - 10_000,
        cpuPercent: 12,
        memoryPercent: 52,
        netReceivedBytes: 1024,
        netSentBytes: 512,
        diskReadBytes: 0,
        diskWriteBytes: 2048,
        gpuPercent: null,
      },
      {
        tsMs: NOW,
        cpuPercent: 18.5,
        memoryPercent: 53,
        netReceivedBytes: 2048,
        netSentBytes: 1024,
        diskReadBytes: 0,
        diskWriteBytes: 0,
        gpuPercent: null,
      },
    ],
    cpu: {
      windowSamples: 174,
      systemAvgPercent: 12,
      othersAvgPercent: 0.4,
      unaccountedAvgPercent: 4.9,
      processes: [cpuProcess('chrome.exe', 3.2, 1), cpuProcess('MsMpEng.exe', 1.4, 2)],
    },
    // No GPU readings by default (a period without GPU counters); the GPU tests give the card its own data.
    gpu: { readingSamples: 0, systemAvgPercent: 0, systemPeakPercent: 0, othersPeakPercent: 0, processes: [] },
    destinations: [
      {
        remoteAddr: '57.144.180.192',
        remotePort: 443,
        bytesSent: 158_970,
        bytesReceived: 23_342_985,
        flowCount: 13,
        processCount: 1,
      },
    ],
    ...overrides,
  };
}

export function processRow(overrides: Partial<ProcessRowView> = {}): ProcessRowView {
  return {
    id: 'p1',
    pid: 10,
    parentId: null,
    name: 'worker.exe',
    imagePath: 'C:\\apps\\worker.exe',
    commandLine: null,
    startedMs: NOW - 60_000,
    endedMs: null,
    endedApproximate: false,
    running: true,
    ...overrides,
  };
}

export function connectionRow(overrides: Partial<ConnectionRowView> = {}): ConnectionRowView {
  return {
    startMs: NOW - 30_000,
    endMs: NOW - 20_000,
    processId: 'p1',
    pid: 10,
    name: 'browser.exe',
    protocol: 'tcp',
    remoteAddr: '203.0.113.9',
    remotePort: 443,
    bytesSent: 2048,
    bytesReceived: 4_194_304,
    opened: 1,
    closed: 0,
    failed: 0,
    ...overrides,
  };
}

export function systemHour(hourMs: number, overrides: Partial<SystemHourView> = {}): SystemHourView {
  return {
    hourMs,
    samples: 120,
    cpuAvgPercent: 12,
    cpuMaxPercent: 35,
    memoryAvgPercent: 52,
    memoryMaxPercent: 58,
    netReceivedBytes: 4096,
    netSentBytes: 1024,
    diskReadBytes: 2048,
    diskWriteBytes: 8192,
    othersCpuAvgPercent: 1,
    gpuAvgPercent: null,
    gpuMaxPercent: null,
    ...overrides,
  };
}

export function systemHistory(overrides: Partial<SystemHistoryView> = {}): SystemHistoryView {
  return {
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    historyStartsMs: NOW - 5 * 86_400_000,
    hours: [systemHour(NOW - 3 * 3_600_000), systemHour(NOW - 2 * 3_600_000, { cpuAvgPercent: 20 })],
    ...overrides,
  };
}

export function gpuProcess(name: string, avg: number, pid: number, memoryPeakBytes = 0): ProcessGpuView {
  return {
    instance: `inst-${pid}`,
    pid,
    name,
    imagePath: `C:\\apps\\${name}`,
    avgPercent: avg,
    peakPercent: Math.min(100, avg * 2),
    memoryPeakBytes,
  };
}
