import { useId, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { IdentityDetailView, StandingView } from '../api/types';

/** Where a lookalike differs from the known product, in plain words. */
function LookalikeNote({ standing }: { standing: StandingView }) {
  const from = standing.expectedPublisher ?? 'that product';
  const where = standing.expectedFolder === null ? '' : ` in ${standing.expectedFolder}`;
  return (
    <div role="note" className="identity__warning">
      <p>
        Looks like {standing.label}, but it is not from {from}
        {where}.
      </p>
      <ul>
        {standing.reasons.map((reason) => (
          <li key={reason}>Why: {reason}</li>
        ))}
      </ul>
    </div>
  );
}

function MarkForm({
  path,
  suggestion,
  onDone,
  onCancel,
}: {
  path: string;
  suggestion: string;
  onDone: () => void;
  onCancel: () => void;
}) {
  const api = useApi();
  const labelId = useId();
  const noteId = useId();
  const [label, setLabel] = useState(suggestion);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const save = async () => {
    setBusy(true);
    setProblem(null);
    const trimmed = note.trim();
    const outcome = await api.markKnown(path, label, trimmed === '' ? undefined : trimmed);
    setBusy(false);
    if (outcome.ok) {
      onDone();
    } else {
      setProblem(outcome.message ?? 'The change could not be saved.');
    }
  };

  return (
    <form
      className="mark-form"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <p>
        <label htmlFor={labelId}>Label</label>
        <input id={labelId} value={label} maxLength={80} onChange={(event) => setLabel(event.target.value)} />
      </p>
      <p>
        <label htmlFor={noteId}>Note (optional)</label>
        <input id={noteId} value={note} maxLength={500} onChange={(event) => setNote(event.target.value)} />
      </p>
      <p className="muted">
        This confirms the product, not only this file: every program from the same maker in the same folder will show as
        known. Programs that claim to be it but are not will be flagged.
      </p>
      {problem !== null && <p role="alert">{problem}</p>}
      <div className="toolbar">
        <button type="submit" className="range__button" disabled={busy}>
          Save
        </button>
        <button type="button" className="range__button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/** What the user has said about this program: known, a lookalike of something known, or not on the list (with a way to add it). */
export function KnownSection({
  name,
  path,
  identity,
  standing,
  onChanged,
}: {
  name: string;
  path: string;
  identity: IdentityDetailView;
  standing: StandingView;
  onChanged: () => void;
}) {
  const [open, setOpen] = useState(false);

  if (standing.state === 'known') {
    return <p>Known program: {standing.label}</p>;
  }
  const suggestion = identity.product ?? identity.publisher ?? name;
  let action;
  if (identity.state !== 'known') {
    action = <p className="muted">A program can be marked as known once its file has been checked.</p>;
  } else if (identity.signature === 'altered') {
    action = <p className="muted">A file that changed since it was signed cannot be marked as known.</p>;
  } else if (open) {
    action = (
      <MarkForm
        path={path}
        suggestion={suggestion}
        onCancel={() => setOpen(false)}
        onDone={() => {
          setOpen(false);
          onChanged();
        }}
      />
    );
  } else {
    action = (
      <button type="button" className="range__button" onClick={() => setOpen(true)}>
        Mark as known
      </button>
    );
  }
  return (
    <>
      {standing.state === 'lookalike' && <LookalikeNote standing={standing} />}
      {action}
    </>
  );
}
