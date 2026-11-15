import type { AddressClass } from './api/types';

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
