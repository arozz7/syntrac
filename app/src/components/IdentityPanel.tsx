import type { ReactNode } from 'react';
import type { IdentityDetailView } from '../api/types';
import { formatBytes, formatDateTime } from '../format';
import { locationLabel, signatureLabel, wasRenamed } from '../identityLabels';
import { SignatureBadge } from './IdentityBadge';

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

const KIND_TEXT = { embedded: 'stored in the file itself', catalog: 'listed in a Windows catalog' } as const;

function FailureNote({ identity }: { identity: IdentityDetailView }) {
  if (identity.failure === null) {
    return null;
  }
  const { kind, message, attempts } = identity.failure;
  const known = identity.state === 'known' ? ' What is shown here is from an earlier look.' : '';
  return (
    <p className="muted">
      {`The last attempt to read this file failed (${kind.replace(/_/g, ' ')}: ${message}; ${attempts} ${
        attempts === 1 ? 'try' : 'tries in a row'
      }).${known}`}
    </p>
  );
}

/** What SYNTRAC found out about one program's file, with plain words for what each fact can and cannot prove. */
export function IdentityPanel({ identity, path }: { identity: IdentityDetailView; path: string }) {
  const label = signatureLabel(identity);
  const known = identity.state === 'known';
  return (
    <section className="identity" aria-label="Identity">
      <h3>Identity</h3>
      <dl className="facts">
        <Fact label="Signature">
          <SignatureBadge identity={identity} />
        </Fact>
        {identity.signer !== null && (
          <Fact label="Signed by">
            {identity.signer}
            {identity.signatureKind !== null && <span className="muted"> ({KIND_TEXT[identity.signatureKind]})</span>}
          </Fact>
        )}
        {identity.thumbprint !== null && <Fact label="Certificate thumbprint">{identity.thumbprint}</Fact>}
        {identity.company !== null && <Fact label="Company">{identity.company}</Fact>}
        {identity.product !== null && <Fact label="Product">{identity.product}</Fact>}
        {identity.description !== null && <Fact label="Description">{identity.description}</Fact>}
        {identity.fileVersion !== null && <Fact label="Version">{identity.fileVersion}</Fact>}
        {identity.originalFilename !== null && <Fact label="Built as">{identity.originalFilename}</Fact>}
        {identity.sha256 !== null && (
          <Fact label="SHA-256">
            <code className="hash">{identity.sha256}</code>
          </Fact>
        )}
        {identity.sizeBytes !== null && <Fact label="File size">{formatBytes(identity.sizeBytes)}</Fact>}
        {identity.modifiedMs !== null && <Fact label="File changed">{formatDateTime(identity.modifiedMs)}</Fact>}
        {identity.inspectedAtMs !== null && <Fact label="Checked">{formatDateTime(identity.inspectedAtMs)}</Fact>}
        {known && (
          <Fact label="Origin">
            {identity.downloaded ? 'Downloaded from the Internet (not unblocked)' : 'No download mark on the file'}
          </Fact>
        )}
        <Fact label="Runs from">
          {locationLabel(identity.location)}
          {identity.installRoot !== null && <span className="muted"> {identity.installRoot}</span>}
        </Fact>
      </dl>
      <p className="muted">{label.hint}</p>
      {known && wasRenamed(path, identity.originalFilename) && (
        <p className="muted">
          This file was built with a different name than it has on disk. That is common (launchers and helper programs
          do it) and is not a warning in itself.
        </p>
      )}
      <FailureNote identity={identity} />
      <p className="muted">
        The signature is the strongest of these signals; the company and product names can be written by anyone, and a
        program can copy itself into a normal-looking folder. None of this proves a program is safe or unsafe: it is
        evidence to weigh.
      </p>
    </section>
  );
}
