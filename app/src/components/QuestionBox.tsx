import { useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { Question, QuestionCall, QuestionStop } from '../api/aiTypes';
import { isFinishedPhase, useJobView } from '../hooks/useJobView';
import { useAiEnabled } from './AiEnabled';

const STOP_TEXT: Partial<Record<QuestionStop, string>> = {
  call_limit: 'It reached the limit of tool calls, so it answered from what it had gathered.',
  time_budget: 'It used up the time set aside for looking things up, so it answered from what it had gathered.',
  context_full: 'The conversation filled the model’s window, so it answered from what it had gathered.',
};

function phaseText(question: Question): string {
  switch (question.phase) {
    case 'waiting':
      return 'Waiting for the model server to be ready…';
    case 'running':
      return question.reasoningChars > 0 && question.answer === ''
        ? `The model is thinking… (${question.reasoningChars} characters of thinking so far.)`
        : 'The model is working on it…';
    case 'done':
      return question.truncated ? 'Finished, but cut off.' : 'Finished.';
    case 'failed':
      return 'It could not finish.';
    case 'cancelled':
      return 'Stopped.';
  }
}

function callText(call: QuestionCall): string {
  const how =
    call.status === 'done'
      ? 'looked up'
      : call.status === 'refused'
        ? `not run${call.note === null ? '' : `: ${call.note}`}`
        : `failed${call.note === null ? '' : `: ${call.note}`}`;
  return `${how}${call.cut ? ' (the result was cut to fit)' : ''}`;
}

/** Every tool call made, in order, so an answer can be checked against what the model was shown. */
function Calls({ calls, finished }: { calls: QuestionCall[]; finished: boolean }) {
  if (calls.length === 0) {
    return finished ? <p className="muted">The model looked nothing up; it answered from the question alone.</p> : null;
  }
  return (
    <>
      <h4>What was looked up</h4>
      <ul className="plain-list" aria-label="What was looked up">
        {calls.map((call, index) => (
          <li key={index}>
            <code>{call.name}</code> <code>{call.arguments}</code> <span className="muted">{callText(call)}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

function Answer({ question }: { question: Question }) {
  const finished = isFinishedPhase(question.phase);
  const stopNote = question.stopped === null ? undefined : STOP_TEXT[question.stopped];
  return (
    <>
      <Calls calls={question.calls} finished={finished} />
      {(question.answer !== '' || finished) && (
        <div className="ai-answer" aria-live="polite" aria-label="Answer">
          {question.answer}
        </div>
      )}
      {stopNote !== undefined && <p role="status">{stopNote}</p>}
      {question.truncated && (
        <p role="status">The model stopped because it reached its length limit, so this answer is cut off.</p>
      )}
      {finished && (
        <p className="muted">
          An answer from a language model can be wrong or incomplete. It is an explanation to help you look, not a
          decision, and it changed nothing: the tools only read.
        </p>
      )}
    </>
  );
}

/** A free-form question for the model server, which may look things up in the recorded history. Nothing is shown while the AI help is off. */
export function QuestionBox() {
  const api = useApi();
  const { enabled } = useAiEnabled();
  const [text, setText] = useState('');
  const [id, setId] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const question = useJobView(id, (number) => api.pollQuestion(number));
  const running = question !== null && !isFinishedPhase(question.phase);

  if (!enabled) {
    return null;
  }

  const send = async () => {
    setProblem(null);
    setStarting(true);
    const result = await api.askQuestion(text);
    setStarting(false);
    if (result.ok) {
      setId(result.value);
    } else {
      setId(null);
      setProblem(result.message);
    }
  };

  return (
    <section className="card card--wide" aria-label="Ask a question">
      <div className="card__head">
        <h2>Ask a question</h2>
      </div>
      <p className="muted">
        Ask in your own words, for example “Which programs were busiest this week?” or “What started widgetsync.exe?”.
        Your model server on this machine may look things up in what SYNTRAC recorded, with a few tools that only read.
        Every lookup is listed with the answer. Nothing leaves your computer.
      </p>
      <label className="ai-field">
        <span>Your question</span>
        <textarea
          rows={3}
          maxLength={500}
          value={text}
          onChange={(event) => setText(event.target.value)}
          disabled={running}
        />
      </label>
      <div className="finding__actions">
        <button
          type="button"
          className="range__button"
          disabled={text.trim() === '' || starting || running}
          onClick={() => void send()}
        >
          Ask
        </button>
        {question !== null && running && (
          <button type="button" className="range__button" onClick={() => void api.cancelQuestion(question.id)}>
            Stop
          </button>
        )}
        {question !== null && <span className="muted">{phaseText(question)}</span>}
      </div>
      {problem !== null && <p role="alert">{problem}</p>}
      {question?.error != null && <p role="alert">{question.error}</p>}
      {question !== null && <Answer question={question} />}
    </section>
  );
}
