import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { useApi } from '../api/ApiContext';

const REFRESH_MS = 5000;

interface AiEnabledValue {
  /** The optional AI help is turned on. Nothing about it is shown while it is off. */
  enabled: boolean;
  /** Reads the switch again now (after the settings were saved). */
  refresh: () => void;
}

const AiEnabledContext = createContext<AiEnabledValue>({ enabled: false, refresh: () => undefined });

/** Reads the AI help switch once for the whole window, so every finding and program does not ask on its own. */
export function AiEnabledProvider({ children }: { children: ReactNode }) {
  const api = useApi();
  const [enabled, setEnabled] = useState(false);
  const [round, setRound] = useState(0);
  const refresh = useCallback(() => setRound((value) => value + 1), []);

  useEffect(() => {
    let cancelled = false;
    const read = () => {
      api
        .aiSettings()
        .then((settings) => {
          if (!cancelled) {
            setEnabled(settings.enabled);
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

  const value = useMemo(() => ({ enabled, refresh }), [enabled, refresh]);
  return <AiEnabledContext.Provider value={value}>{children}</AiEnabledContext.Provider>;
}

export function useAiEnabled(): AiEnabledValue {
  return useContext(AiEnabledContext);
}
