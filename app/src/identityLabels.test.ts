import { describe, expect, it } from 'vitest';
import { locationLabel, locationNote, signatureLabel, wasRenamed } from './identityLabels';
import { identityBadge } from './test/fakeApi';

describe('signatureLabel', () => {
  it('says plainly what is known and what is not', () => {
    expect(signatureLabel(identityBadge({ state: 'pending', signature: null }))).toMatchObject({
      text: 'Not yet checked',
      tone: 'quiet',
    });
    expect(signatureLabel(identityBadge({ state: 'unreadable', signature: null }))).toMatchObject({
      text: 'Could not read the file',
      tone: 'caution',
    });
    expect(signatureLabel(identityBadge({ signature: 'valid' }))).toMatchObject({ text: 'Signed', tone: 'good' });
    expect(signatureLabel(identityBadge({ signature: 'untrusted' }))).toMatchObject({
      text: 'Signed, not trusted',
      tone: 'caution',
    });
    expect(signatureLabel(identityBadge({ signature: 'altered' }))).toMatchObject({
      text: 'Changed since signed',
      tone: 'alert',
    });
  });

  it('does not treat an unsigned program as bad: most developer tools and store apps are unsigned', () => {
    expect(signatureLabel(identityBadge({ signature: 'unsigned' }))).toMatchObject({ text: 'Unsigned', tone: 'quiet' });
  });

  it('gives every label a hint that says what the signal can and cannot prove', () => {
    for (const signature of ['valid', 'untrusted', 'altered', 'unsigned'] as const) {
      expect(signatureLabel(identityBadge({ signature })).hint.length).toBeGreaterThan(20);
    }
    expect(signatureLabel(identityBadge({ signature: 'valid' })).hint).toMatch(/not.*safe/i);
  });
});

describe('locationNote', () => {
  it('mentions only the places worth a second look', () => {
    expect(locationNote('temporary')).toBe('runs from a temporary folder');
    expect(locationNote('user_profile')).toBe('runs from a user folder');
    expect(locationNote('network')).toBe('runs from a network share');
    for (const quiet of ['system', 'program_files', 'program_data', 'other', 'unknown'] as const) {
      expect(locationNote(quiet)).toBeNull();
    }
  });

  it('names every location in full for the panel', () => {
    expect(locationLabel('program_files')).toBe('Program Files');
    expect(locationLabel('temporary')).toBe('Temporary folder');
    expect(locationLabel('unknown')).toBe('Unknown');
  });
});

describe('wasRenamed', () => {
  it('compares the built-in name with the name on disk, ignoring case', () => {
    expect(wasRenamed('C:\\Apps\\Tool.exe', 'tool.EXE')).toBe(false);
    expect(wasRenamed('C:\\Apps\\tool.exe', 'other.exe')).toBe(true);
    expect(wasRenamed('C:\\Apps\\tool.exe', null)).toBe(false);
  });
});
