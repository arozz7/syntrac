import { useCallback, useEffect, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { InvestigateSubject, Investigation, PromptPreview, SavedAnswer } from '../api/aiTypes';
import { formatDateTime } from '../format';
import { isFinishedPhase, useJobView } from '../hooks/useJobView';
import { subjectKey, useAiEnabled } from './AiEnabled';

function isFinished(investigation: Investigation | null): boolean {
  return investigation !== null && isFinishedPhase(investigation.phase);
}

const PHASE_TEXT: Record<string, string> = {
  waiting: 'Waiting for the model server to be ready…',
  running: 'The model is answering…',
  done: 'Finished.',
  failed: 'It could not finish.',
  cancelled: 'Stopped.',
};

/** One line for where the investigation is. A reasoning model thinks for a while before its first words, so that is said, with how much. */
function phaseText(investigation: Investigation): string {
  if (investigation.phase === 'done' && investigation.truncated) {
    return 'Finished, but cut off.';
  }
  if (investigation.phase === 'running' && investigation.answer === '' && investigation.reasoningChars > 0) {
    return `The model is thinking… (${investigation.reasoningChars} characters of thinking so far). A model that thinks first can take a minute or two before its first words.`;
  }
  return PHASE_TEXT[investigation.phase] ?? '';
}

/** An answer as plain text, with what to keep in mind about it and what was sent to get it. */
function AnswerBlock({
  answer,
  truncated,
  level,
  sent,
}: {
  answer: string;
  truncated: boolean;
  level: string;
  sent: string;
}) {
  return (
    <>
      <div className="ai-answer" aria-live="polite" aria-label="Answer">
        {answer}
      </div>
      {truncated && (
        <p role="status">
          The model stopped because it reached its length limit, so this answer is cut off. A model that thinks first
          can use much of the limit on its thinking. You can ask again.
        </p>
      )}
      <p className="muted">
        An answer from a language model can be wrong or incomplete. It is an explanation to help you look, not a
        decision, and it changed nothing.
      </p>
      <details>
        <summary>What was sent ({level} detail)</summary>
        <pre className="ai-sent">{sent}</pre>
      </details>
    </>
  );
}

/** The answer kept from an earlier time for this subject (null when there is none), with a way to read it again and to drop it. */
function useKeptAnswer(subject: InvestigateSubject): {
  kept: SavedAnswer | null;
  reload: () => void;
  drop: () => void;
} {
  const api = useApi();
  const key = subjectKey(subject);
  const [kept, setKept] = useState<SavedAnswer | null>(null);
  const [round, setRound] = useState(0);
  const reload = useCallback(() => setRound((value) => value + 1), []);

  useEffect(() => {
    let cancelled = false;
    api
      .savedAnswer(subject)
      .then((answer) => {
        if (!cancelled) {
          setKept(answer);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
    // The subject object is made again on every render of its page; its key says whether it is another one.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, key, round]);

  return { kept, reload, drop: () => setKept(null) };
}

function LocalAnswer({ subject }: { subject: InvestigateSubject }) {
  const api = useApi();
  const { refresh } = useAiEnabled();
  const [id, setId] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const { kept, reload, drop } = useKeptAnswer(subject);
  // A finished answer was kept by the program: read it again, and let the lists show their mark.
  const keptNow = useCallback(() => {
    reload();
    refresh();
  }, [reload, refresh]);
  const investigation = useJobView(id, (number) => api.pollInvestigation(number), keptNow);
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

  const forget = async () => {
    await api.forgetAnswer(subject);
    drop();
    refresh();
  };

  const showKept = investigation === null && kept !== null;
  return (
    <section aria-label="Ask my model server">
      <h4>Ask my model server</h4>
      <p className="muted">
        Sends a short summary of the evidence to the model server on this machine and shows its answer. Nothing leaves
        your computer.
      </p>
      <div className="finding__actions">
        <button type="button" className="range__button" disabled={starting || running} onClick={() => void run()}>
          {id === null && kept === null ? 'Investigate with my model' : 'Ask again'}
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
        {kept !== null && !running && (
          <button type="button" className="range__button" onClick={() => void forget()}>
            Forget the saved answer
          </button>
        )}
        {investigation !== null && <span className="muted">{phaseText(investigation)}</span>}
      </div>
      {problem !== null && <p role="alert">{problem}</p>}
      {investigation?.error != null && <p role="alert">{investigation.error}</p>}
      {showKept && (
        <>
          <p>
            <strong>Saved answer from {formatDateTime(kept.savedMs)}</strong>{' '}
            <span className="muted">
              (model {kept.model}). It was kept so the model need not be asked again; nothing was sent just now.
            </span>
          </p>
          <AnswerBlock answer={kept.answer} truncated={kept.truncated} level={kept.level} sent={kept.sent} />
        </>
      )}
      {investigation !== null && (
        <AnswerBlock
          answer={investigation.answer}
          truncated={investigation.truncated}
          level={investigation.level}
          sent={investigation.sent}
        />
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
