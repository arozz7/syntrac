import type { IdentityBadgeView } from '../api/types';
import { locationNote, signatureLabel } from '../identityLabels';

/** The signature state as a small label. The words say what is known; the tooltip says what it can and cannot prove. */
export function SignatureBadge({ identity }: { identity: IdentityBadgeView }) {
  const label = signatureLabel(identity);
  return (
    <span className={`badge badge--${label.tone}`} title={label.hint}>
      {label.text}
    </span>
  );
}

/** An unsigned file has no signer: its publisher is a company name anyone can write into the file. */
function publisherHint(identity: IdentityBadgeView): string {
  return identity.signature === 'unsigned' || identity.signature === null
    ? "The company named in the file's version information. Anyone can write this, so it is a label, not proof."
    : 'Signed by this name.';
}

/** For a table cell: the signature label, who made the program, and a note when it runs from an unusual place. */
export function IdentityCell({ identity }: { identity: IdentityBadgeView }) {
  const note = locationNote(identity.location);
  return (
    <div className="identity-cell">
      <SignatureBadge identity={identity} />
      {identity.publisher !== null && (
        <span className="identity-cell__publisher" title={publisherHint(identity)}>
          {identity.publisher}
        </span>
      )}
      {note !== null && <span className="muted">{note}</span>}
    </div>
  );
}
