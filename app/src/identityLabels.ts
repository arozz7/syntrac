import type { IdentityBadgeView, LocationClass } from './api/types';

/** How loudly to show a label: `alert` is for a file that no longer matches its signature, never for merely unsigned. */
export type Tone = 'good' | 'caution' | 'alert' | 'quiet';

export interface SignatureLabel {
  text: string;
  tone: Tone;
  /** What this signal can and cannot prove, for a tooltip. */
  hint: string;
}

const NOT_PROOF = 'A signature shows who signed the file, not that the program is safe.';

/** The words for a program's signature state. Plain, and honest about what is not known. */
export function signatureLabel(identity: IdentityBadgeView): SignatureLabel {
  if (identity.state === 'pending') {
    return {
      text: 'Not yet checked',
      tone: 'quiet',
      hint: 'SYNTRAC has not looked at this file yet. Files are checked a few at a time in the background.',
    };
  }
  if (identity.state === 'unreadable') {
    return {
      text: 'Could not read the file',
      tone: 'caution',
      hint: 'The file could not be opened (it may have been removed), so nothing is known about it. This is not the same as unsigned.',
    };
  }
  switch (identity.signature) {
    case 'valid':
      return {
        text: 'Signed',
        tone: 'good',
        hint: `The signature is valid and the signing certificate is trusted by Windows. ${NOT_PROOF}`,
      };
    case 'untrusted':
      return {
        text: 'Signed, not trusted',
        tone: 'caution',
        hint: 'The file is signed but Windows does not trust the certificate (unknown, expired or revoked). Checked offline, so a certificate that needs a missing intermediate is also reported this way.',
      };
    case 'altered':
      return {
        text: 'Changed since signed',
        tone: 'alert',
        hint: 'The file was signed but its contents no longer match the signature: it was modified after signing.',
      };
    default:
      return {
        text: 'Unsigned',
        tone: 'quiet',
        hint: 'The file carries no signature. Many ordinary programs are unsigned (developer tools, some free software); this alone says little.',
      };
  }
}

const NOTES: Partial<Record<LocationClass, string>> = {
  temporary: 'runs from a temporary folder',
  user_profile: 'runs from a user folder',
  network: 'runs from a network share',
};

/** A short note for a table row, only for locations worth a second look. */
export function locationNote(location: LocationClass): string | null {
  return NOTES[location] ?? null;
}

const NAMES: Record<LocationClass, string> = {
  system: 'Windows folder',
  program_files: 'Program Files',
  program_data: 'ProgramData',
  user_profile: 'User profile folder',
  temporary: 'Temporary folder',
  network: 'Network share',
  other: 'Other folder',
  unknown: 'Unknown',
};

export function locationLabel(location: LocationClass): string {
  return NAMES[location];
}

/** True when the name the file was built with differs from its name on disk. Information only: it is common and harmless. */
export function wasRenamed(path: string, originalFilename: string | null): boolean {
  if (originalFilename === null) {
    return false;
  }
  const onDisk = path.split(/[\\/]/).pop() ?? path;
  return onDisk.toLowerCase() !== originalFilename.toLowerCase();
}
