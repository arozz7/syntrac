import { describe, expect, it } from 'vitest';
import {
  accountConcern,
  accountLabel,
  locationLabel,
  locationNote,
  signatureLabel,
  wasRenamed,
} from './identityLabels';
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

describe('accountLabel', () => {
  it('names each kind of account in plain words', () => {
    expect(accountLabel('system')).toBe('SYSTEM account');
    expect(accountLabel('local_service')).toBe('Local Service account');
    expect(accountLabel('network_service')).toBe('Network Service account');
    expect(accountLabel('service_account')).toBe('Service account');
    expect(accountLabel('user')).toBe('user account');
    expect(accountLabel('other')).toBe('other account');
  });
});

describe('accountConcern', () => {
  const system = { runsAs: ['system' as const] };

  it('is only raised for a privileged account running from a temporary folder, a user folder or a network share', () => {
    expect(accountConcern(identityBadge({ ...system, location: 'temporary' }))).toMatch(/worth a look/i);
    expect(accountConcern(identityBadge({ ...system, location: 'user_profile' }))).toMatch(/worth a look/i);
    expect(accountConcern(identityBadge({ ...system, location: 'network' }))).toMatch(/worth a look/i);
  });

  it('is quiet for the ordinary cases', () => {
    expect(accountConcern(identityBadge({ ...system, location: 'program_files' }))).toBeNull();
    expect(accountConcern(identityBadge({ ...system, location: 'system' }))).toBeNull();
    expect(accountConcern(identityBadge({ runsAs: ['user'], location: 'temporary' }))).toBeNull();
    expect(accountConcern(identityBadge({ runsAs: [], location: 'temporary' }))).toBeNull();
  });

  it('counts the other service accounts as privileged too', () => {
    for (const kind of ['local_service', 'network_service', 'service_account'] as const) {
      expect(accountConcern(identityBadge({ runsAs: [kind], location: 'temporary' }))).not.toBeNull();
    }
  });
});
