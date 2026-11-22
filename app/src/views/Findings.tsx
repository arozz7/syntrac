import { useEffect, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type {
  DetectionView,
  DigestView,
  ExpectationView,
  FindingCountsView,
  FindingDecision,
  FindingStatus,
  FindingTier,
  FindingView,
  FindingsView,
} from '../api/types';
import { DataGate } from '../components/DataGate';
import { InvestigateButton } from '../components/InvestigatePanel';
import { SavedNote } from '../components/SavedNote';
import { formatBytes, formatDateTime } from '../format';
import { usePolled } from '../hooks/usePolled';
import { ReviewToday } from './ReviewToday';

const REFRESH_MS = 30_000;

const FILTERS: Array<{ id: FindingStatus | undefined; label: string; count?: keyof FindingCountsView }> = [
  { id: 'open', label: 'Open', count: 'open' },
  { id: 'not_expected', label: 'Not expected', count: 'notExpected' },
  { id: 'snoozed', label: 'Snoozed', count: 'snoozed' },
  { id: 'expected', label: 'Expected', count: 'expected' },
  { id: undefined, label: 'All' },
];

const TIERS: Record<FindingTier, { label: string; shape: string; what: string }> = {
  unusual: { label: 'Unusual', shape: '●', what: 'differs from what was seen before' },
  suspicious: { label: 'Suspicious', shape: '▲', what: 'matches a pattern often seen in rogue software' },
  confirmed: { label: 'Confirmed', shape: '■', what: 'matches a list you imported' },
};

const KINDS: Record<string, string> = {
  new_program: 'New program',
  new_start_entry: 'New start entry',
  changed_file: 'Changed file',
  new_account: 'New account',
  new_owner: 'New network owner',
  usage: 'Usage above what is expected',
};

const PATTERN_PREFIX = 'pattern_';

function kindLabel(kind: string): string {
  if (kind.startsWith(PATTERN_PREFIX)) {
    return `Known pattern: ${kind.slice(PATTERN_PREFIX.length).replace(/_/g, ' ')}`;
  }
  return KINDS[kind] ?? kind.replace(/_/g, ' ');
}

/** What a raised limit allows, in the unit of what the rule measures (the end of the subject says which). */
function ceilingText(expectation: ExpectationView): string {
  const ceiling = expectation.ceiling ?? 0;
  if (expectation.subject.endsWith('|cpu')) {
    return `Up to ${ceiling.toFixed(1)}%`;
  }
  // The GPU figure is a share of the program's busiest engine, not of the machine.
  if (expectation.subject.endsWith('|gpu')) {
    return `Up to ${ceiling.toFixed(1)}% of one GPU engine`;
  }
  return `Up to ${formatBytes(ceiling)}`;
}

function DetectionNote({ detection }: { detection: DetectionView | null }) {
  if (detection === null) {
    return (
      <p className="muted">
        Detection has not run yet. It is off until it is switched on in the service configuration. When it first runs,
        that moment is when &quot;new&quot; starts counting: everything that exists then is already here and is not
        raised.
      </p>
    );
  }
  return (
    <>
      <p className="muted">
        Detection has been on since {formatDateTime(detection.switchedOnMs)}; things first seen before then count as
        already here.{' '}
        {detection.lastRunMs === null
          ? 'It has not finished a run yet.'
          : `It last ran ${formatDateTime(detection.lastRunMs)}.`}
      </p>
      {detection.lastError !== null && <p role="alert">The last run had a problem: {detection.lastError}</p>}
    </>
  );
}

function Filters({
  status,
  counts,
  onChange,
}: {
  status: FindingStatus | undefined;
  counts: FindingCountsView;
  onChange: (status: FindingStatus | undefined) => void;
}) {
  return (
    <div className="range" role="group" aria-label="Show findings that are">
      {FILTERS.map((filter) => (
        <button
          key={filter.label}
          type="button"
          className="range__button"
          aria-pressed={status === filter.id}
          onClick={() => onChange(filter.id)}
        >
          {filter.count === undefined ? filter.label : `${filter.label} (${counts[filter.count]})`}
        </button>
      ))}
    </div>
  );
}

function statusLine(finding: FindingView): string | null {
  switch (finding.status) {
    case 'open':
      return null;
    case 'expected':
      return 'You said this is expected.';
    case 'not_expected':
      return 'You said this is not expected. It stays here until you decide otherwise.';
    case 'snoozed':
      return finding.snoozedUntilMs === null ? 'Snoozed.' : `Snoozed until ${formatDateTime(finding.snoozedUntilMs)}.`;
    case 'resolved':
      return 'No longer happening.';
  }
}

function times(hits: number): string {
  return hits === 1 ? 'found once' : `found ${hits} times`;
}

function FindingCard({ finding, onChanged }: { finding: FindingView; onChanged: () => void }) {
  const api = useApi();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const [note, setNote] = useState('');
  const [scope, setScope] = useState<'item' | 'folder' | 'build'>('item');
  const [days, setDays] = useState('7');
  const tier = TIERS[finding.tier];
  const line = statusLine(finding);

  const decide = async (
    decision: FindingDecision,
    options?: { days?: number; note?: string; scope?: 'this_folder' | 'build_folder' },
  ) => {
    setBusy(true);
    setProblem(null);
    const outcome = await api.decideFinding(finding.id, decision, options);
    setBusy(false);
    if (outcome.ok) {
      setAsking(false);
      onChanged();
    } else {
      setProblem(outcome.message ?? 'The change could not be saved.');
    }
  };

  return (
    <article className="finding">
      <div className="finding__head">
        <span className={`chip chip--${finding.tier}`} title={`${tier.label}: ${tier.what}`}>
          <span aria-hidden="true">{tier.shape}</span> {tier.label}
        </span>
        <h3>{finding.title}</h3>
        <SavedNote subject={{ kind: 'finding', id: finding.id }} />
      </div>
      <p>{finding.why}</p>
      {finding.evidence.length > 0 && (
        <ul className="names">
          {finding.evidence.map((fact) => (
            <li key={fact}>{fact}</li>
          ))}
        </ul>
      )}
      <InvestigateButton subject={{ kind: 'finding', id: finding.id }} />
      <p className="muted">
        This was {times(finding.hits)}: first {formatDateTime(finding.firstAtMs)}, last{' '}
        {formatDateTime(finding.lastAtMs)}.
      </p>
      {problem !== null && <p role="alert">{problem}</p>}
      {line !== null ? (
        <div className="finding__actions">
          <span className="muted">{line}</span>
          <button type="button" className="range__button" disabled={busy} onClick={() => void decide('reopen')}>
            Reopen
          </button>
        </div>
      ) : asking ? (
        <div className="finding__actions">
          <p className="muted">Only this kind of finding is silenced, and only for what you choose here.</p>
          {(finding.folderScope !== null || finding.buildScope !== null) && (
            <fieldset className="finding__scope">
              <legend className="muted">What should this cover?</legend>
              <label>
                <input
                  type="radio"
                  name={`scope-${finding.id}`}
                  checked={scope === 'item'}
                  onChange={() => setScope('item')}
                />{' '}
                Only this program
              </label>
              {finding.folderScope !== null && (
                <label>
                  <input
                    type="radio"
                    name={`scope-${finding.id}`}
                    checked={scope === 'folder'}
                    onChange={() => setScope('folder')}
                  />{' '}
                  Everything below {finding.folderScope}
                </label>
              )}
              {finding.buildScope !== null && (
                <>
                  <label>
                    <input
                      type="radio"
                      name={`scope-${finding.id}`}
                      checked={scope === 'build'}
                      onChange={() => setScope('build')}
                    />{' '}
                    New programs in the build folder {finding.buildScope}
                  </label>
                  <p className="muted">
                    For a tool or project folder you write programs into. A program copied there by anything else would
                    not be called new either; it is counted in the summary above, and every other rule still applies.
                  </p>
                </>
              )}
            </fieldset>
          )}
          <input
            type="text"
            className="field"
            aria-label="Note (optional)"
            placeholder="Note (optional), for example what it is"
            maxLength={200}
            value={note}
            onChange={(event) => setNote(event.target.value)}
          />
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => {
              const options = {
                ...(note.trim() === '' ? {} : { note: note.trim() }),
                ...(scope === 'folder' && finding.folderScope !== null ? { scope: 'this_folder' as const } : {}),
                ...(scope === 'build' && finding.buildScope !== null ? { scope: 'build_folder' as const } : {}),
              };
              void decide('always_expected', Object.keys(options).length === 0 ? undefined : options);
            }}
          >
            Save as always expected
          </button>
          <button type="button" className="range__button" disabled={busy} onClick={() => setAsking(false)}>
            Cancel
          </button>
        </div>
      ) : (
        <div className="finding__actions">
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void decide('expected_this_time')}
          >
            Expected this time
          </button>
          {finding.kind === 'usage' ? (
            <button type="button" className="range__button" disabled={busy} onClick={() => void decide('raise_limit')}>
              Raise the limit to this level
            </button>
          ) : (
            <button type="button" className="range__button" disabled={busy} onClick={() => setAsking(true)}>
              Always expected
            </button>
          )}
          <button type="button" className="range__button" disabled={busy} onClick={() => void decide('not_expected')}>
            Not expected
          </button>
          <select aria-label="Snooze for" value={days} onChange={(event) => setDays(event.target.value)}>
            <option value="1">1 day</option>
            <option value="7">7 days</option>
            <option value="30">30 days</option>
          </select>
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void decide('snooze', { days: Number(days) })}
          >
            Snooze
          </button>
        </div>
      )}
    </article>
  );
}

/** Remove, with a question first: it brings the finding back the next time the rule finds it. */
function RemoveButton({ expectation, onChanged }: { expectation: ExpectationView; onChanged: () => void }) {
  const api = useApi();
  const [asking, setAsking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const remove = async () => {
    const outcome = await api.removeExpectation(expectation.id);
    if (outcome.ok) {
      setAsking(false);
      onChanged();
    } else {
      setProblem(outcome.message ?? 'The change could not be saved.');
    }
  };

  if (!asking) {
    return (
      <button
        type="button"
        className="range__button"
        aria-label={`Remove ${kindLabel(expectation.kind)} for ${expectation.subject}`}
        onClick={() => setAsking(true)}
      >
        Remove
      </button>
    );
  }
  return (
    <div>
      <p>Raise this again when it is found?</p>
      {problem !== null && <p role="alert">{problem}</p>}
      <button type="button" className="range__button" onClick={() => void remove()}>
        Yes, remove
      </button>{' '}
      <button
        type="button"
        className="range__button"
        onClick={() => {
          setAsking(false);
          setProblem(null);
        }}
      >
        Keep
      </button>
    </div>
  );
}

function Expectations({ expectations, onChanged }: { expectations: ExpectationView[]; onChanged: () => void }) {
  return (
    <section className="card card--wide">
      <h2>What you told SYNTRAC is expected</h2>
      <p className="muted">
        Each line covers one kind of finding about exactly one program or entry, nothing wider. Remove one and it is
        raised again the next time it is found.
      </p>
      {expectations.length === 0 ? (
        <p className="muted">You have not marked anything as expected yet.</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>Kind</th>
              <th>Covers</th>
              <th>Note</th>
              <th>Added</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {expectations.map((expectation) => (
              <tr key={expectation.id}>
                <td>{kindLabel(expectation.kind)}</td>
                <td className="path">
                  {expectation.scope === 'folder' && <>Everything below {expectation.scopeRoot}</>}
                  {expectation.scope === 'build' && <>New programs in the build folder {expectation.scopeRoot}</>}
                  {expectation.scope === 'item' && expectation.subject}
                  {expectation.writableWarning && (
                    <div>
                      Made before whole folders were limited to Windows and Program Files: a program copied there is
                      never called new. Remove it to be told about new programs here.
                    </div>
                  )}
                </td>
                <td>
                  {expectation.note ?? (expectation.ceiling === null ? '—' : null)}
                  {expectation.ceiling !== null && <div>{ceilingText(expectation)}</div>}
                </td>
                <td>{formatDateTime(expectation.createdMs)}</td>
                <td>
                  <RemoveButton expectation={expectation} onChanged={onChanged} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

const LAST_LOOKED_KEY = 'syntrac.findings.lastLookedMs';

/** When the tab was last left, as a per-viewer convenience: the browser may refuse storage, and nothing depends on it. */
function readLastLooked(): number | null {
  try {
    const stored = window.localStorage.getItem(LAST_LOOKED_KEY);
    return stored === null ? null : Number(stored);
  } catch {
    return null;
  }
}

function writeLastLooked(ms: number) {
  try {
    window.localStorage.setItem(LAST_LOOKED_KEY, String(ms));
  } catch {
    // Nothing depends on it.
  }
}

function Digest({ digest }: { digest: DigestView }) {
  return (
    <>
      <p className="muted">
        In the last {digest.windowHours} hours: {digest.newDestinations} new internet{' '}
        {digest.newDestinations === 1 ? 'address' : 'addresses'}
        {digest.ownerData
          ? ` and ${digest.newOwners} ${digest.newOwners === 1 ? 'network' : 'networks'} never reached before`
          : '. Download the owner data (History, Network) to count the networks too'}
        . Counted, not raised: new destinations appear every day; see them under History, Network.
      </p>
      {digest.newInBuildFolders > 0 && (
        <p className="muted">
          {digest.newInBuildFolders} new {digest.newInBuildFolders === 1 ? 'program' : 'programs'} appeared in your
          build folders in that time. Not raised, because you said new programs there are expected; they are still
          listed under Programs.
        </p>
      )}
    </>
  );
}

function FindingsBody({
  view,
  status,
  newSince,
  onStatus,
  onChanged,
}: {
  view: FindingsView;
  /** How many open findings appeared since the tab was last looked at; null the first time. */
  newSince: number | null;
  status: FindingStatus | undefined;
  onStatus: (status: FindingStatus | undefined) => void;
  onChanged: () => void;
}) {
  return (
    <>
      <section className="card card--wide">
        <DetectionNote detection={view.detection} />
        <Digest digest={view.digest} />
        {newSince !== null && newSince > 0 && <p role="status">{newSince} new since you last looked.</p>}
        <p className="muted">
          A finding is evidence to look at, not a verdict: the tiers are unusual (differs from what was seen before),
          suspicious (matches a pattern often seen in rogue software) and confirmed (matches a list you imported). Tell
          SYNTRAC what is expected and it stops raising it.
        </p>
        <Filters status={status} counts={view.counts} onChange={onStatus} />
        {view.findings.length === 0 ? (
          <p className="muted">Nothing here: no findings with this status.</p>
        ) : (
          view.findings.map((finding) => <FindingCard key={finding.id} finding={finding} onChanged={onChanged} />)
        )}
      </section>
      <ReviewToday onChanged={onChanged} />
      <Expectations expectations={view.expectations} onChanged={onChanged} />
    </>
  );
}

/** What the detection rules found, and the place to say what is expected. */
export function Findings() {
  const api = useApi();
  const [status, setStatus] = useState<FindingStatus | undefined>('open');
  // Bumped after a decision, so the list is read again at once instead of at the next refresh.
  const [version, setVersion] = useState(0);
  // When the tab was last left is read once, when it opens, and written when it closes.
  const [lastLooked] = useState(readLastLooked);
  useEffect(() => () => writeLastLooked(Date.now()), []);
  const result = usePolled(() => api.findings(status), REFRESH_MS, `findings:${status ?? 'all'}:${version}`);
  return (
    <DataGate result={result}>
      {(view) => (
        <FindingsBody
          view={view}
          status={status}
          newSince={
            lastLooked === null
              ? null
              : view.findings.filter((f) => f.status === 'open' && f.firstAtMs > lastLooked).length
          }
          onStatus={setStatus}
          onChanged={() => setVersion((v) => v + 1)}
        />
      )}
    </DataGate>
  );
}
