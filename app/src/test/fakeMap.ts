// Sample data for the connection map (see fakeApi.ts).

import type { ConnectionMapView } from '../api/types';
import { NOW } from './fakeHome';

/** chrome.exe and code.exe talk to EXAMPLE-NET (two addresses, one with a name); code.exe also to a network nobody owns. */
export function connectionMapView(overrides: Partial<ConnectionMapView> = {}): ConnectionMapView {
  return {
    fromMs: NOW - 7 * 86_400_000,
    toMs: NOW,
    historyStartsMs: NOW - 16 * 86_400_000,
    programs: [
      {
        kind: 'program',
        name: 'chrome.exe',
        path: String.raw`C:\Program Files\Google\Chrome\chrome.exe`,
        programs: 1,
        bytesSent: 110,
        bytesReceived: 5_000_000,
      },
      {
        kind: 'program',
        name: 'code.exe',
        path: String.raw`C:\Apps\code.exe`,
        programs: 1,
        bytesSent: 1_000_000,
        bytesReceived: 2_000,
      },
    ],
    networks: [
      {
        kind: 'owner',
        label: 'EXAMPLE-NET Example Networks, Inc.',
        owner: { asn: 64500, org: 'EXAMPLE-NET Example Networks, Inc.', country: 'US' },
        networks: 1,
        addresses: 2,
        bytesSent: 1_000_110,
        bytesReceived: 5_001_000,
        top: [
          { addr: '203.0.113.9', name: 'www.example.com' },
          { addr: '203.0.113.10', name: null },
        ],
      },
      {
        kind: 'no_owner',
        label: 'No owner found',
        owner: null,
        networks: 1,
        addresses: 1,
        bytesSent: 0,
        bytesReceived: 1_000,
        top: [{ addr: '198.51.100.7', name: null }],
      },
    ],
    links: [
      { program: 0, network: 0, bytesSent: 110, bytesReceived: 4_999_000 },
      { program: 1, network: 0, bytesSent: 1_000_000, bytesReceived: 2_000 },
      { program: 0, network: 1, bytesSent: 0, bytesReceived: 1_000 },
    ],
    totalPrograms: 2,
    totalNetworks: 2,
    ownerData: { source: 'iptoasn.com', downloadedMs: NOW - 3 * 86_400_000, rows: 480_000 },
    ...overrides,
  };
}
