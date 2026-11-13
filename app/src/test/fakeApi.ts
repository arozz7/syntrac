import { vi } from 'vitest';
import type { Api } from '../api/api';
import type { ConnectionRowView, Data, HealthView, OverviewView, ProcessCpuView, ProcessRowView } from '../api/types';

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
      },
      {
        tsMs: NOW,
        cpuPercent: 18.5,
        memoryPercent: 53,
        netReceivedBytes: 2048,
        netSentBytes: 1024,
        diskReadBytes: 0,
        diskWriteBytes: 0,
      },
    ],
    cpu: {
      windowSamples: 174,
      systemAvgPercent: 12,
      othersAvgPercent: 0.4,
      unaccountedAvgPercent: 4.9,
      processes: [cpuProcess('chrome.exe', 3.2, 1), cpuProcess('MsMpEng.exe', 1.4, 2)],
    },
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

const ok = <T>(data: T): Data<T> => ({ state: 'ok', data });

/** An `Api` whose every method is a spy returning sensible sample data. */
export function makeFakeApi(overrides: Partial<Api> = {}): Api & { [K in keyof Api]: ReturnType<typeof vi.fn> } {
  return {
    health: vi.fn(async () => health()),
    overview: vi.fn(async () => ok(overview())),
    processes: vi.fn(async () =>
      ok([
        processRow(),
        processRow({ id: 'p2', pid: 11, name: 'gone.exe', running: false, endedMs: NOW - 1000 }),
        processRow({
          id: 'p3',
          pid: 12,
          name: 'lost.exe',
          running: false,
          endedMs: NOW - 2000,
          endedApproximate: true,
        }),
      ]),
    ),
    connections: vi.fn(async () =>
      ok([
        connectionRow(),
        connectionRow({ name: 'pid 99', pid: 99, processId: null, protocol: 'udp', remotePort: 5353 }),
      ]),
    ),
    ...overrides,
  } as Api & { [K in keyof Api]: ReturnType<typeof vi.fn> };
}
