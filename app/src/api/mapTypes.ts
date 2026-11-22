// Views of the connection map: which programs talked to which networks (see types.ts).
import type { OwnerDataView, OwnerView } from './networkTypes';

export interface MapProgramView {
  kind: 'program' | 'unidentified' | 'others';
  name: string;
  /** The program's image path, which opens its details; null for an unidentified or folded-together node. */
  path: string | null;
  /** How many programs the node stands for. */
  programs: number;
  bytesSent: number;
  bytesReceived: number;
}

export interface MapAddressView {
  addr: string;
  /** The name asked for most recently, when any was recorded. */
  name: string | null;
}

export interface MapNetworkView {
  kind: 'owner' | 'no_owner' | 'others';
  label: string;
  owner: OwnerView | null;
  /** How many networks the node stands for. */
  networks: number;
  addresses: number;
  bytesSent: number;
  bytesReceived: number;
  /** The biggest addresses of the network, which open their details. */
  top: MapAddressView[];
}

export interface MapLinkView {
  /** Indexes into `programs` and `networks`. */
  program: number;
  network: number;
  bytesSent: number;
  bytesReceived: number;
}

export interface ConnectionMapView {
  fromMs: number;
  toMs: number;
  historyStartsMs: number | null;
  programs: MapProgramView[];
  networks: MapNetworkView[];
  links: MapLinkView[];
  /** How many programs and networks there were before the cut. */
  totalPrograms: number;
  totalNetworks: number;
  ownerData: OwnerDataView | null;
}
