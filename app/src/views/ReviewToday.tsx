import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { ReviewGroupView } from '../api/types';
import { DataGate } from '../components/DataGate';
import { formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';

const REFRESH_MS = 60_000;

function sentence(concerns: string[]): string {
  const text = concerns.join(' and ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function Group({ group, onChanged }: { group: ReviewGroupView; onChanged: () => void }) {
  const api = useApi();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const first = group.programs[0];

  const run = async (work: () => Promise<{ ok: boolean; message: string | null }>) => {
    setBusy(true);
    setProblem(null);
    const outcome = await work();
    setBusy(false);
    if (outcome.ok) {
      onChanged();
    } else {
      setProblem(outcome.message ?? 'The change could not be saved.');
    }
  };

  return (
    <article className="finding">
      <div className="finding__head">
        <h3>{group.label}</h3>
        <span className="muted">
          {group.count} {group.count === 1 ? 'program' : 'programs'} in {group.location}
        </span>
      </div>
      {group.concerns.length > 0 && <p>Worth a look: {sentence(group.concerns)}.</p>}
      <ul className="names">
        {group.programs.map((program) => (
          <li key={program.path} title={program.path}>
            {program.name}
          </li>
        ))}
        {group.count > group.programs.length && <li className="muted">+{group.count - group.programs.length} more</li>}
      </ul>
      <p className="muted">Folder: {group.root}</p>
      {!group.wholeFolderAllowed && (
        <p className="muted">
          Anything can be copied into this folder, so only the programs of this group are marked: a program that appears
          later is still raised.
        </p>
      )}
      {problem !== null && <p role="alert">{problem}</p>}
      <div className="finding__actions">
        {group.wholeFolderAllowed ? (
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void run(() => api.confirmReview(group.root))}
          >
            Mark as expected
          </button>
        ) : (
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void run(() => api.confirmReviewPrograms(group.root, group.publisher))}
          >
            {group.count === 1 ? 'Mark this program as expected' : `Mark these ${group.count} programs as expected`}
          </button>
        )}
        {first !== undefined && (
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void run(() => api.markKnown(first.path, group.label))}
          >
            Mark as a known program
          </button>
        )}
      </div>
    </article>
  );
}

function ReviewBody({ onChanged }: { onChanged: () => void }) {
  const api = useApi();
  const [version, setVersion] = useState(0);
  const result = usePolled(() => api.review(), REFRESH_MS, `review:${version}`);
  const changed = () => {
    setVersion((current) => current + 1);
    onChanged();
  };
  return (
    <DataGate result={result}>
      {(view) => (
        <>
          <p className="muted">
            {view.switchedOnMs === null
              ? 'Detection has not been switched on yet, so this is every program seen running so far.'
              : `These programs were already here when detection was switched on (${formatDateTime(view.switchedOnMs)}), so they are never raised as new.`}{' '}
            Look at the ones marked, and say what is expected. For Windows and Program Files, new programs in a folder
            you mark are not raised either; for any other folder only the programs listed here are marked.
          </p>
          {view.groups.length === 0 ? (
            <p className="muted">Nothing left to review.</p>
          ) : (
            view.groups.map((group) => (
              <Group key={group.root + (group.publisher ?? '')} group={group} onChanged={changed} />
            ))
          )}
          {view.totalGroups > view.groups.length && (
            <p className="muted">
              Showing {view.groups.length} of {view.totalGroups} groups.
            </p>
          )}
        </>
      )}
    </DataGate>
  );
}

/** What was here when detection was switched on, grouped by product, to look at once. Reads nothing until it is opened. */
export function ReviewToday({ onChanged }: { onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <section className="card card--wide">
      <div className="card__head">
        <h2>What was here when detection was switched on</h2>
        <button
          type="button"
          className="range__button"
          aria-expanded={open}
          onClick={() => setOpen((current) => !current)}
        >
          {open ? 'Hide' : 'Review what was here'}
        </button>
      </div>
      {open && <ReviewBody onChanged={onChanged} />}
    </section>
  );
}
