import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useApi } from '../api/ApiContext';
import type { InvestigateSubject } from '../api/aiTypes';

const REFRESH_MS = 5000;

/** What makes two subjects the same: a Windows path is the same program whatever its letter case. */
export function subjectKey(subject: InvestigateSubject): string {
  return subject.kind === 'program' ? `program:${subject.path.toLowerCase()}` : `finding:${subject.id}`;
}

interface AiEnabledValue {
  /** The optional AI help is turned on. Nothing about it is shown while it is off. */
  enabled: boolean;
  /** When an answer was kept for each subject (milliseconds since 1970), by `subjectKey`. Empty while the help is off. */
  saved: ReadonlyMap<string, number>;
  /** Reads the switch and the kept answers again now (after the settings were saved, or an answer was kept or forgotten). */
  refresh: () => void;
}

const NONE: ReadonlyMap<string, number> = new Map();

const AiEnabledContext = createContext<AiEnabledValue>({ enabled: false, saved: NONE, refresh: () => undefined });

/** Reads the AI help switch once for the whole window, so every finding and program does not ask on its own. */
export function AiEnabledProvider({ children }: { children: ReactNode }) {
  const api = useApi();
  const [enabled, setEnabled] = useState(false);
  const [saved, setSaved] = useState<ReadonlyMap<string, number>>(NONE);
  const [round, setRound] = useState(0);
  const refresh = useCallback(() => setRound((value) => value + 1), []);

  useEffect(() => {
    let cancelled = false;
    const read = () => {
      api
        .aiSettings()
        .then(async (settings) => {
          if (cancelled) {
            return;
          }
          setEnabled(settings.enabled);
          // The kept answers are read only while the help is on.
          const marks = settings.enabled ? await api.savedAnswers() : [];
          if (!cancelled) {
            setSaved(new Map(marks.map((mark) => [subjectKey(mark.subject), mark.savedMs])));
          }
        })
        .catch(() => {
          // Keep the last known switch.
        });
    };
    read();
    const timer = setInterval(read, REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [api, round]);

  const value = useMemo(() => ({ enabled, saved, refresh }), [enabled, saved, refresh]);
  return <AiEnabledContext.Provider value={value}>{children}</AiEnabledContext.Provider>;
}

export function useAiEnabled(): AiEnabledValue {
  return useContext(AiEnabledContext);
}
