import { describe, expect, it } from 'vitest';
import { classLabel, nameLine } from './networkLabels';

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
