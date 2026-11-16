import { describe, expect, it } from 'vitest';
import { classLabel, dataAgeDays, nameLine, ownerLine } from './networkLabels';

describe('classLabel', () => {
  it('says nothing about an address on the internet: that is the ordinary case', () => {
    expect(classLabel('public')).toBeNull();
  });

  it('names the other kinds of address in plain words', () => {
    expect(classLabel('private')).toBe('This network');
    expect(classLabel('link_local')).toBe('Same link only');
    expect(classLabel('multicast')).toBe('Multicast');
    expect(classLabel('other')).toBe('Special address');
  });
});

describe('nameLine', () => {
  it('says plainly when no name was recorded', () => {
    expect(nameLine(null, 0)).toBe('No name seen');
  });

  it('shows the name alone when it is the only one', () => {
    expect(nameLine('calendar.google.com', 0)).toBe('calendar.google.com');
  });

  it('counts the other names, with the right plural', () => {
    expect(nameLine('calendar.google.com', 1)).toBe('calendar.google.com (1 other name)');
    expect(nameLine('calendar.google.com', 2)).toBe('calendar.google.com (2 other names)');
  });
});

describe('ownerLine', () => {
  it('names the network, who runs it and where it is registered', () => {
    expect(ownerLine({ asn: 13335, org: 'CLOUDFLARENET - Cloudflare, Inc.', country: 'US' })).toBe(
      'Network AS13335 CLOUDFLARENET - Cloudflare, Inc., registered in US',
    );
  });

  it('leaves out what it does not know instead of inventing it', () => {
    expect(ownerLine({ asn: 64500, org: '', country: 'ZZ' })).toBe('Network AS64500');
  });
});

describe('dataAgeDays', () => {
  const DAY = 86_400_000;

  it('counts whole days', () => {
    expect(dataAgeDays(10 * DAY, 10 * DAY + 3 * DAY + 5000)).toBe(3);
    expect(dataAgeDays(10 * DAY, 10 * DAY + 5000)).toBe(0);
  });

  it('is never negative when the clock is behind', () => {
    expect(dataAgeDays(10 * DAY, 9 * DAY)).toBe(0);
  });
});
