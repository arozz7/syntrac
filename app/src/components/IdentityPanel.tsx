import type { ReactNode } from 'react';
import type { IdentityDetailView, StandingView } from '../api/types';
import { formatBytes, formatDateTime } from '../format';
import {
  accountConcern,
  accountLabel,
  launcherLabel,
  registeredDate,
  locationLabel,
  signatureLabel,
  wasRenamed,
} from '../identityLabels';
import { SignatureBadge } from './IdentityBadge';
import { KnownSection } from './KnownSection';

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
export function IdentityPanel({
  identity,
  standing,
  name,
  path,
  onChanged,
}: {
  identity: IdentityDetailView;
  standing: StandingView;
  name: string;
  path: string;
  onChanged: () => void;
}) {
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
        <Fact label="Runs as">
          {identity.accounts.length === 0 ? (
            <span className="muted">Not observed running yet</span>
          ) : (
            <ul className="names">
              {identity.accounts.map((account) => (
                <li key={`${account.kind}:${account.name ?? ''}`}>
                  <code>{account.name ?? 'unnamed account'}</code>{' '}
                  <span className="muted">
                    ({accountLabel(account.kind)}), seen {formatDateTime(account.firstSeenMs)} to{' '}
                    {formatDateTime(account.lastSeenMs)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Fact>
        {identity.services.length > 0 && (
          <Fact label="Services">
            <ul className="names">
              {identity.services.map((service) => (
                <li key={service.name}>
                  <code>{service.name}</code>
                  {service.displayName !== null && <span className="muted"> {service.displayName}</span>}
                </li>
              ))}
            </ul>
          </Fact>
        )}
        <Fact label="Registered by an installer as">
          {identity.registered.length === 0 ? (
            <span className="muted">No installer record names this folder</span>
          ) : (
            <ul className="names">
              {identity.registered.map((product) => (
                <li key={`${product.name}:${product.folder}`}>
                  <strong>{product.name}</strong>
                  <span className="muted">
                    {product.publisher !== null && ` from ${product.publisher}`}
                    {product.version !== null && `, version ${product.version}`}
                    {product.installed !== null && `, installed ${registeredDate(product.installed)}`}
                  </span>
                  {!product.current && <span className="muted"> (no longer installed)</span>}
                  {product.folderDerived && (
                    <span className="muted"> (folder worked out from the uninstall command, not stated)</span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </Fact>
        <Fact label="Starts with Windows via">
          {identity.launchers.length === 0 ? (
            <span className="muted">
              No start-up entry found in the services, scheduled tasks, Run keys and Startup folders that were read
            </span>
          ) : (
            <ul className="names">
              {identity.launchers.map((entry) => (
                <li key={`${entry.kind}:${entry.location}`}>
                  {launcherLabel(entry.kind, entry.detail)} <code>{entry.location}</code>
                  {entry.kind === 'task' && entry.detail !== '' && <span className="muted"> ({entry.detail})</span>}
                  {entry.arguments !== '' && <span className="muted"> {entry.arguments}</span>}
                  <span className="muted">
                    {' '}
                    since {formatDateTime(entry.firstSeenMs)}
                    {!entry.current && ', no longer configured'}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Fact>
      </dl>
      <p className="muted">{label.hint}</p>
      {accountConcern(identity) !== null && <p className="muted">{accountConcern(identity)}</p>}
      <p className="muted">
        An installer record is a clue to where a program came from, not proof: any program that can write the registry
        can claim a name, and it never changes whether a file is signed or known.
      </p>
      <p className="muted">
        Not read: Winlogon settings, debugger and DLL hooks, WMI subscriptions, drivers, users who are not signed in,
        and programs started by another program. A program can start in those ways, so "no entry found" only means "not
        found in the places read".
      </p>
      <p className="muted">
        The account shows how a program was started and what it is allowed to do, not whether it is safe. It is recorded
        only while the program is seen running, so a program that runs briefly can be missed.
      </p>
      {known && wasRenamed(path, identity.originalFilename) && (
        <p className="muted">
          This file was built with a different name than it has on disk. That is common (launchers and helper programs
          do it) and is not a warning in itself.
        </p>
      )}
      <FailureNote identity={identity} />
      <KnownSection name={name} path={path} identity={identity} standing={standing} onChanged={onChanged} />
      <p className="muted">
        The signature is the strongest of these signals; the company and product names can be written by anyone, and a
        program can copy itself into a normal-looking folder. None of this proves a program is safe or unsafe: it is
        evidence to weigh.
      </p>
    </section>
  );
}
