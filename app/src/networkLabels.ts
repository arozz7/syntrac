import type { AddressClass, OwnerView } from './api/types';

/** Words for the kind of address. Nothing for an address on the internet: that is the ordinary case, so it is not labelled. */
export function classLabel(addressClass: AddressClass): string | null {
  switch (addressClass) {
    case 'public':
      return null;
    case 'private':
      return 'This network';
    case 'link_local':
      return 'Same link only';
    case 'multicast':
      return 'Multicast';
    case 'other':
      return 'Special address';
  }
}

/** The name a program asked for when it reached an address, and how many other names the address has. */
export function nameLine(name: string | null, otherNames: number): string {
  if (name === null) {
    return 'No name seen';
  }
  if (otherNames === 0) {
    return name;
  }
  return `${name} (${otherNames} other ${otherNames === 1 ? 'name' : 'names'})`;
}

/**
 * Who runs the network range an address is in, and where that network is registered. Said as a clue, never as the service you
 * reached: a cloud or hosting company owns the network of thousands of unrelated services.
 */
export function ownerLine(owner: OwnerView): string {
  const network = owner.org === '' ? `Network AS${owner.asn}` : `Network AS${owner.asn} ${owner.org}`;
  return owner.country === 'ZZ' || owner.country === '' ? network : `${network}, registered in ${owner.country}`;
}

/** Whole days between `downloadedMs` and `nowMs` (0 when the clock is behind). */
export function dataAgeDays(downloadedMs: number, nowMs: number): number {
  return Math.max(0, Math.floor((nowMs - downloadedMs) / 86_400_000));
}
