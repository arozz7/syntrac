import { useEffect, useRef, useState } from 'react';

/**
 * Loads once, then again every `intervalMs`, and whenever `key` changes (for example the selected range).
 * The previous value stays visible while a refresh is in flight, so the screen does not flicker.
 * `load` may change on every render; only `key` and `intervalMs` restart the polling.
 */
export function usePolled<T>(load: () => Promise<T>, intervalMs: number, key: string): T | undefined {
  const [value, setValue] = useState<T>();
  const latestLoad = useRef(load);
  useEffect(() => {
    latestLoad.current = load;
  });

  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      latestLoad
        .current()
        .then((next) => {
          if (!cancelled) {
            setValue(next);
          }
        })
        .catch(() => {
          // A failed refresh keeps the last good value; the API layer already turns errors into states.
        });
    };
    refresh();
    const timer = setInterval(refresh, intervalMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [intervalMs, key]);

  return value;
}
