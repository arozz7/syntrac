import type { ReactNode } from 'react';
import type { Data } from '../api/types';

/** Shows the data when there is some, and an explanation (never a blank screen) when there is not. */
export function DataGate<T>({ result, children }: { result: Data<T> | undefined; children: (data: T) => ReactNode }) {
  if (!result) {
    return <p className="muted">Loading…</p>;
  }
  switch (result.state) {
    case 'ok':
      return <>{children(result.data)}</>;
    case 'no_database':
      return (
        <section className="notice">
          <h2>No data yet</h2>
          <p>{result.message}</p>
          <p className="muted">
            Install the collector service with scripts/deploy/install-service.ps1, or start a console collector and
            point this app at its database with SYNTRAC_DB.
          </p>
        </section>
      );
    case 'schema_mismatch':
      return (
        <section className="notice">
          <h2>Database version mismatch</h2>
          <p>{result.message}</p>
        </section>
      );
    case 'error':
      return (
        <section className="notice">
          <h2>Could not read the data</h2>
          <p>{result.message}</p>
        </section>
      );
  }
}
