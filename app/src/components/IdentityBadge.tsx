import type { IdentityBadgeView, StandingView } from '../api/types';
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

/** Where a program stands against the user's known programs. Nothing is shown for a program that is not on the list. */
export function StandingBadge({ standing }: { standing: StandingView }) {
  if (standing.state === 'known') {
    return <span className="badge badge--good">Known: {standing.label}</span>;
  }
  if (standing.state === 'lookalike') {
    return (
      <span className="badge badge--alert" title={standing.reasons.join('; ')}>
        Looks like {standing.label}
      </span>
    );
  }
  return null;
}

/** For a table cell: the signature label, who made the program, and a note when it runs from an unusual place. */
export function IdentityCell({ identity, standing }: { identity: IdentityBadgeView; standing?: StandingView }) {
  const note = locationNote(identity.location);
  return (
    <div className="identity-cell">
      <SignatureBadge identity={identity} />
      {standing !== undefined && <StandingBadge standing={standing} />}
      {identity.service && (
        <span className="badge badge--quiet" title="This program has been seen hosting a Windows service.">
          Service
        </span>
      )}
      {identity.startsAtBoot && (
        <span
          className="badge badge--quiet"
          title="Something that starts with Windows (an automatic service, a scheduled task, a Run key or a Startup folder entry) was found for this program."
        >
          Starts at boot
        </span>
      )}
      {identity.publisher !== null && (
        <span className="identity-cell__publisher" title={publisherHint(identity)}>
          {identity.publisher}
        </span>
      )}
      {note !== null && <span className="muted">{note}</span>}
    </div>
  );
}
