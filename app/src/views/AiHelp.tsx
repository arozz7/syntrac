import { useEffect, useState } from 'react';
import { useApi } from '../api/ApiContext';
import type { AiDetailLevel, AiSettings } from '../api/aiTypes';
import { useAiEnabled } from '../components/AiEnabled';
import { usePolled } from '../hooks/usePolled';

const STATUS_REFRESH_MS = 3000;

function argsToText(args: string[]): string {
  return args.join('\n');
}

function textToArgs(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.replace(/\r$/, ''))
    .filter((line) => line.trim() !== '');
}

function ServerStatus({ version, onChanged }: { version: number; onChanged: () => void }) {
  const api = useApi();
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const status = usePolled(() => api.aiStatus(), STATUS_REFRESH_MS, `ai-status-${version}`);

  const act = async (action: () => Promise<{ ok: boolean; message?: string }>) => {
    setBusy(true);
    setProblem(null);
    const result = await action();
    setBusy(false);
    if (!result.ok) {
      setProblem(result.message ?? 'That did not work.');
    }
    onChanged();
  };

  if (status === undefined || status === null) {
    return <p className="muted">The state of the model server is not known yet.</p>;
  }
  return (
    <div aria-label="Model server status">
      <p>
        <strong>{status.message}</strong>
        {status.seconds !== null && <span className="muted"> ({status.seconds} s)</span>}
      </p>
      {status.command !== null && (
        <p className="muted">
          The command <em>Start</em> runs, exactly: <code>{status.command}</code>
        </p>
      )}
      {problem !== null && <p role="alert">{problem}</p>}
      <div className="finding__actions">
        {status.canStart && (
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void act(() => api.startAiServer())}
          >
            Start the model server
          </button>
        )}
        {status.canStop && (
          <button
            type="button"
            className="range__button"
            disabled={busy}
            onClick={() => void act(() => api.stopAiServer())}
          >
            Stop the model server
          </button>
        )}
      </div>
      <p className="muted">
        SYNTRAC only stops a server it started itself. One that was already running is left alone.
      </p>
    </div>
  );
}

function Form({ initial, onSaved }: { initial: AiSettings; onSaved: () => void }) {
  const api = useApi();
  const [draft, setDraft] = useState<AiSettings>(initial);
  const [args, setArgs] = useState(argsToText(initial.launchArgs));
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const change = (patch: Partial<AiSettings>) => {
    setDraft((current) => ({ ...current, ...patch }));
    setMessage(null);
  };

  const save = async () => {
    const result = await api.saveAiSettings({ ...draft, launchArgs: textToArgs(args) });
    if (result.ok) {
      setMessage({ ok: true, text: 'Saved.' });
      onSaved();
    } else {
      setMessage({ ok: false, text: result.message });
    }
  };

  return (
    <form
      className="ai-form"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <label className="ai-check">
        <input
          type="checkbox"
          checked={draft.enabled}
          onChange={(event) => change({ enabled: event.target.checked })}
        />
        Turn on AI help
      </label>

      <label className="ai-field">
        <span>Model server address</span>
        <input
          type="text"
          value={draft.endpoint}
          placeholder="http://127.0.0.1:1234/v1"
          onChange={(event) => change({ endpoint: event.target.value })}
        />
        <span className="muted">Only an address on this machine is accepted (127.0.0.1, localhost or [::1]).</span>
      </label>

      <label className="ai-field">
        <span>Model name</span>
        <input type="text" value={draft.model} onChange={(event) => change({ model: event.target.value })} />
        <span className="muted">Sent exactly as typed. Some servers switch models when the name changes.</span>
      </label>

      <fieldset className="ai-field">
        <legend>How much is sent to your model server</legend>
        {(['summary', 'full'] as AiDetailLevel[]).map((level) => (
          <label key={level} className="ai-check">
            <input
              type="radio"
              name="detail"
              checked={draft.detailLevel === level}
              onChange={() => change({ detailLevel: level })}
            />
            {level === 'summary'
              ? 'Summary: names, networks and numbers; no addresses, hashes or account names'
              : 'Full: also addresses, file hashes and account names'}
          </label>
        ))}
        <span className="muted">
          Your user name is always replaced. A server may keep a log of every prompt in plain text.
        </span>
      </fieldset>

      <fieldset className="ai-field">
        <legend>Starting the server from SYNTRAC (optional)</legend>
        <p className="muted">
          Leave empty if you start your model server yourself (LM Studio, llama-server or another). The program is run
          directly, with no shell, only when you press Start.
        </p>
        <label className="ai-field">
          <span>Program (a full path to an .exe)</span>
          <input
            type="text"
            value={draft.launchProgram}
            onChange={(event) => change({ launchProgram: event.target.value })}
          />
        </label>
        <label className="ai-field">
          <span>Arguments (one per line)</span>
          <textarea
            rows={3}
            value={args}
            onChange={(event) => {
              setArgs(event.target.value);
              setMessage(null);
            }}
          />
        </label>
        <label className="ai-field">
          <span>Working folder (optional)</span>
          <input
            type="text"
            value={draft.launchWorkingDir}
            onChange={(event) => change({ launchWorkingDir: event.target.value })}
          />
        </label>
        <label className="ai-check">
          <input
            type="checkbox"
            checked={draft.startOnInvestigate}
            onChange={(event) => change({ startOnInvestigate: event.target.checked })}
          />
          Start the server when I press Investigate and it is not running
        </label>
        <label className="ai-check">
          <input
            type="checkbox"
            checked={draft.stopOnQuit}
            onChange={(event) => change({ stopOnQuit: event.target.checked })}
          />
          Stop a server SYNTRAC started when SYNTRAC quits
        </label>
      </fieldset>

      <div className="finding__actions">
        <button type="submit" className="range__button">
          Save
        </button>
        {message !== null && <span role={message.ok ? 'status' : 'alert'}>{message.text}</span>}
      </div>
    </form>
  );
}

export function AiHelp() {
  const api = useApi();
  const { refresh } = useAiEnabled();
  const [initial, setInitial] = useState<AiSettings | null>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api
      .aiSettings()
      .then((settings) => {
        if (!cancelled) {
          setInitial(settings);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [api]);

  const changed = () => {
    setVersion((value) => value + 1);
    refresh();
  };

  return (
    <div className="grid">
      <section className="card card--wide">
        <div className="card__head">
          <h2>AI help (optional)</h2>
        </div>
        <p>
          An <strong>Investigate</strong> button on findings and programs asks a language model to explain the evidence
          in plain words. It is off until you turn it on. The model runs on this machine: SYNTRAC talks only to a server
          on 127.0.0.1 and never to the Internet. Detection never uses it, and it never changes anything.
        </p>
        {initial === null ? (
          <p className="muted">Loading the settings…</p>
        ) : (
          <Form initial={initial} onSaved={changed} />
        )}
      </section>
      <section className="card card--wide">
        <div className="card__head">
          <h2>Model server</h2>
        </div>
        <ServerStatus version={version} onChanged={changed} />
      </section>
    </div>
  );
}
