import { useEffect, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { InvestigateSubject, Investigation, PromptPreview } from '../api/aiTypes';
import { useAiEnabled } from './AiEnabled';

const POLL_MS = 700;

function isFinished(investigation: Investigation | null): boolean {
  return investigation !== null && ['done', 'failed', 'cancelled'].includes(investigation.phase);
}

/** Reads one investigation every moment until it ends. */
function useInvestigation(id: number | null): Investigation | null {
  const api = useApi();
  const [view, setView] = useState<Investigation | null>(null);

  useEffect(() => {
    if (id === null) {
      return undefined;
    }
    let cancelled = false;
    let finished = false;
    const read = () => {
      api
        .pollInvestigation(id)
        .then((next) => {
          if (!cancelled) {
            setView(next);
            finished = isFinished(next) || next === null;
          }
        })
        .catch(() => undefined);
    };
    read();
    const timer = setInterval(() => {
      if (finished) {
        clearInterval(timer);
        return;
      }
      read();
    }, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [api, id]);

  return id === null ? null : view;
}

const PHASE_TEXT: Record<string, string> = {
  waiting: 'Waiting for the model server to be ready…',
  running: 'The model is answering…',
  done: 'Finished.',
  failed: 'It could not finish.',
  cancelled: 'Stopped.',
};

function LocalAnswer({ subject }: { subject: InvestigateSubject }) {
  const api = useApi();
  const [id, setId] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const investigation = useInvestigation(id);
  const running = investigation !== null && !isFinished(investigation);

  const run = async () => {
    setProblem(null);
    setStarting(true);
    const result = await api.startInvestigation(subject);
    setStarting(false);
    if (result.ok) {
      setId(result.value);
    } else {
      setId(null);
      setProblem(result.message);
    }
  };

  return (
    <section aria-label="Ask my model server">
      <h4>Ask my model server</h4>
      <p className="muted">
        Sends a short summary of the evidence to the model server on this machine and shows its answer. Nothing leaves
        your computer.
      </p>
      <div className="finding__actions">
        <button type="button" className="range__button" disabled={starting || running} onClick={() => void run()}>
          {id === null ? 'Investigate with my model' : 'Ask again'}
        </button>
        {investigation !== null && running && (
          <button
            type="button"
            className="range__button"
            onClick={() => void api.cancelInvestigation(investigation.id)}
          >
            Stop
          </button>
        )}
        {investigation !== null && <span className="muted">{PHASE_TEXT[investigation.phase]}</span>}
      </div>
      {problem !== null && <p role="alert">{problem}</p>}
      {investigation?.error != null && <p role="alert">{investigation.error}</p>}
      {investigation !== null && (
        <>
          <div className="ai-answer" aria-live="polite" aria-label="Answer">
            {investigation.answer}
          </div>
          <p className="muted">
            An answer from a language model can be wrong or incomplete. It is an explanation to help you look, not a
            decision, and it changed nothing.
          </p>
          <details>
            <summary>What was sent ({investigation.level} detail)</summary>
            <pre className="ai-sent">{investigation.sent}</pre>
          </details>
        </>
      )}
    </section>
  );
}

function OutsideCopy({ subject }: { subject: InvestigateSubject }) {
  const api = useApi();
  const [preview, setPreview] = useState<PromptPreview | null>(null);
  const [text, setText] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const [copied, setCopied] = useState<'yes' | 'no' | null>(null);

  const prepare = async () => {
    setProblem(null);
    setCopied(null);
    const result = await api.previewPrompt(subject, true);
    if (result.ok) {
      setPreview(result.value);
      setText(result.value.text);
    } else {
      setProblem(result.message);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied('yes');
    } catch {
      setCopied('no');
    }
  };

  return (
    <section aria-label="Copy a prompt for an outside assistant">
      <h4>Copy a prompt for an outside assistant</h4>
      <p className="muted">
        Prepares a reduced text with no file names, folders, addresses or finding sentences. You can edit it. SYNTRAC
        sends nothing anywhere: pasting it into a hosted assistant is your own action and shares it with that service.
      </p>
      <div className="finding__actions">
        <button type="button" className="range__button" onClick={() => void prepare()}>
          {preview === null ? 'Show the text to copy' : 'Reset the text'}
        </button>
      </div>
      {problem !== null && <p role="alert">{problem}</p>}
      {preview !== null && (
        <>
          <label className="ai-field">
            <span className="muted">Exactly what will be copied</span>
            <textarea
              className="ai-copy"
              rows={14}
              value={text}
              onChange={(event) => {
                setText(event.target.value);
                setCopied(null);
              }}
            />
          </label>
          <div className="finding__actions">
            <button type="button" className="range__button" onClick={() => void copy()}>
              Copy to clipboard
            </button>
            {copied === 'yes' && <span role="status">Copied.</span>}
            {copied === 'no' && <span role="alert">Could not copy. Select the text and copy it yourself.</span>}
          </div>
        </>
      )}
    </section>
  );
}

function Panel({ subject, onClose }: { subject: InvestigateSubject; onClose: () => void }) {
  return (
    <div className="ai-panel" role="region" aria-label="Investigate">
      <LocalAnswer subject={subject} />
      <OutsideCopy subject={subject} />
      <div className="finding__actions">
        <button type="button" className="range__button" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}

/** The Investigate button of a finding or a program. Nothing is shown while the AI help is turned off. */
export function InvestigateButton({ subject }: { subject: InvestigateSubject }) {
  const { enabled } = useAiEnabled();
  const [open, setOpen] = useState(false);
  if (!enabled) {
    return null;
  }
  return (
    <div className="ai-investigate">
      <button type="button" className="range__button" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
        Investigate
      </button>
      {open && <Panel subject={subject} onClose={() => setOpen(false)} />}
    </div>
  );
}
