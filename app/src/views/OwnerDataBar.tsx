import { useEffect, useRef, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { OwnerDataOutcome, OwnerDataView } from '../api/types';
import { Dialog } from '../components/Dialog';
import { formatDay } from '../format';
import { dataAgeDays } from '../networkLabels';

const IDLE_POLL_MS = 30_000;
const ACTIVE_POLL_MS = 2_000;
/** Data older than this is mentioned, so the owners shown are not silently out of date. */
const OLD_AFTER_DAYS = 30;

function ConfirmDownload({ onDownload, onCancel }: { onDownload: () => void; onCancel: () => void }) {
  return (
    <Dialog title="Download owner data?" onClose={onCancel}>
      <p>
        SYNTRAC will download one public-domain file from iptoasn.com (a few MB up to tens of MB, never more than 64
        MB). It says which network owns each range of internet addresses.
      </p>
      <p>It sends no address of yours: the lookups happen on this machine.</p>
      <p>Nothing else is downloaded, and it happens only when you press Download.</p>
      <div className="toolbar">
        <button type="button" className="range__button" onClick={onDownload}>
          Download
        </button>
        <button type="button" className="range__button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </Dialog>
  );
}

function Stored({ dataset, nowMs }: { dataset: OwnerDataView; nowMs: number }) {
  const age = dataAgeDays(dataset.downloadedMs, nowMs);
  return (
    <>
      <p>
        Owner data: downloaded {formatDay(dataset.downloadedMs)} from {dataset.source}.
      </p>
      <p className="muted">{dataset.rows.toLocaleString('en-US')} network ranges.</p>
      {age >= OLD_AFTER_DAYS && <p className="muted">This data is {age} days old.</p>}
    </>
  );
}

/**
 * Whether the machine has the owner data (which network owns an address), with the one button that downloads it. The download is
 * the only thing SYNTRAC ever fetches from the internet, so it asks first and says exactly what it does.
 *
 * It looks at the service now and then (quickly while a download is under way, slowly otherwise) and tells the screen when new
 * data has arrived.
 */
export function OwnerDataBar({
  nowMs,
  onDownloaded,
  activePollMs = ACTIVE_POLL_MS,
}: {
  nowMs: number;
  /** Called when a download has finished and new owner data is on the machine, so the screen can read it again. */
  onDownloaded: () => void;
  activePollMs?: number;
}) {
  const api = useApi();
  const [confirming, setConfirming] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [status, setStatus] = useState<OwnerDataOutcome>();
  // Changes when the user agreed, so the loop below looks again at once.
  const [kick, setKick] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const look = async () => {
      let outcome: OwnerDataOutcome;
      try {
        outcome = await api.ownerDataStatus();
      } catch {
        outcome = { ok: false, state: null, message: 'The owner data could not be checked.' };
      }
      if (cancelled) {
        return;
      }
      setStatus(outcome);
      timer = setTimeout(() => void look(), outcome.state?.refreshing === true ? activePollMs : IDLE_POLL_MS);
    };
    void look();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [api, activePollMs, kick]);

  const state = status?.ok === true ? status.state : null;
  const downloadedAt = useRef<number | null | undefined>(undefined);
  useEffect(() => {
    if (state === null) {
      return;
    }
    const stamp = state.dataset?.downloadedMs ?? null;
    if (downloadedAt.current !== undefined && downloadedAt.current !== stamp) {
      onDownloaded();
    }
    downloadedAt.current = stamp;
  }, [state, onDownloaded]);

  const download = async () => {
    setConfirming(false);
    setRefusal(null);
    const outcome = await api.refreshOwnerData();
    if (!outcome.ok) {
      setRefusal(outcome.message ?? 'The download could not be started.');
      return;
    }
    setStatus(outcome);
    setKick((count) => count + 1);
  };

  let body;
  if (status === undefined) {
    body = <p className="muted">Checking the owner data…</p>;
  } else if (!status.ok) {
    body = <p className="muted">{status.message ?? 'The owner data could not be checked.'}</p>;
  } else if (state === null) {
    body = <p className="muted">The owner data could not be checked.</p>;
  } else if (!state.allowed) {
    body = <p className="muted">Downloading owner data is switched off in the service&apos;s configuration.</p>;
  } else if (state.refreshing) {
    body = <p>Downloading owner data…</p>;
  } else {
    body = (
      <>
        {state.dataset === null ? (
          <>
            <p>Owner data: not downloaded.</p>
            <p className="muted">Without it, an address shows only the names programs asked for.</p>
          </>
        ) : (
          <Stored dataset={state.dataset} nowMs={nowMs} />
        )}
        {state.lastError !== null && <p className="muted">The last attempt failed: {state.lastError}</p>}
        <button type="button" className="range__button" onClick={() => setConfirming(true)}>
          {state.dataset === null ? 'Download owner data' : 'Refresh owner data'}
        </button>
      </>
    );
  }

  return (
    <section className="card card--wide">
      <h2>Owner data</h2>
      {body}
      {refusal !== null && <p className="muted">{refusal}</p>}
      {confirming && <ConfirmDownload onDownload={() => void download()} onCancel={() => setConfirming(false)} />}
    </section>
  );
}
