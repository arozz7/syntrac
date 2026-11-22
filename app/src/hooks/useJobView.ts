import { useEffect, useRef, useState } from 'react';

const POLL_MS = 700;

interface Job {
  phase: string;
}

export function isFinishedPhase(phase: string): boolean {
  return ['done', 'failed', 'cancelled'].includes(phase);
}

/**
 * Reads one background job (an investigation, a question) every moment until it ends. `onDone` is called once, when the job
 * finishes with an answer. `poll` may change on every render; the latest one is used.
 */
export function useJobView<T extends Job>(
  id: number | null,
  poll: (id: number) => Promise<T | null>,
  onDone: () => void = () => undefined,
): T | null {
  const [view, setView] = useState<T | null>(null);
  const latest = useRef({ poll, onDone });
  useEffect(() => {
    latest.current = { poll, onDone };
  }, [poll, onDone]);

  useEffect(() => {
    if (id === null) {
      return undefined;
    }
    let cancelled = false;
    let finished = false;
    const read = () => {
      latest.current
        .poll(id)
        .then((next) => {
          if (!cancelled) {
            setView(next);
            const wasFinished = finished;
            finished = next === null || isFinishedPhase(next.phase);
            if (!wasFinished && next?.phase === 'done') {
              latest.current.onDone();
            }
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
  }, [id]);

  return id === null ? null : view;
}
