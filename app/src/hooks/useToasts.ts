import { useEffect, useRef } from 'react';
import type { FindingView } from '../api/types';
import { planToasts } from '../toasts';

/**
 * Raises a toast for each new suspicious finding while the app runs. `open` is the latest list of open findings (undefined while
 * it cannot be read). The ids seen are kept even while toasts are off, so switching them on does not announce what is old.
 */
export function useToasts(
  open: readonly FindingView[] | undefined,
  enabled: boolean,
  notify: (title: string, body: string) => Promise<void>,
) {
  const seen = useRef<ReadonlySet<number> | null>(null);
  useEffect(() => {
    if (open === undefined) {
      return;
    }
    const plan = planToasts(open, seen.current);
    seen.current = plan.seen;
    if (enabled) {
      for (const toast of plan.toasts) {
        void notify(toast.title, toast.body);
      }
    }
  }, [open, enabled, notify]);
}
