// Views of the network: destinations, the networks that own them, and what each program connected to (see types.ts).

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
