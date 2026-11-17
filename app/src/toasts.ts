import type { FindingView } from './api/types';

/** A Windows notification: a short title and a body of at most 200 characters. */
export interface Toast {
  title: string;
  body: string;
}

export interface ToastPlan {
  toasts: Toast[];
  /** The ids of the findings that are open now: what the next plan compares with. */
  seen: Set<number>;
}

const MAX_INDIVIDUAL = 3;
const MAX_BODY = 200;
const SEE_THEM = 'Open the Findings tab to see them.';

function cut(text: string): string {
  return text.length <= MAX_BODY ? text : `${text.slice(0, MAX_BODY - 1)}…`;
}

/**
 * Which toasts a fresh list of open findings calls for, given the ids already seen. Pure, so the rules are tested once:
 * - the first list (seen is null) shows nothing: what is already open is not news (the sidebar count and "new since you last looked" say it);
 * - one toast for each new suspicious or confirmed finding, at most three, then one line saying how many more;
 * - the unusual ones are one summary, never one each;
 * - the digest is never a toast (it is not a finding).
 */
export function planToasts(open: readonly FindingView[], seen: ReadonlySet<number> | null): ToastPlan {
  const current = open.filter((finding) => finding.status === 'open');
  const next = new Set(current.map((finding) => finding.id));
  if (seen === null) {
    return { toasts: [], seen: next };
  }
  const fresh = current.filter((finding) => !seen.has(finding.id));
  const serious = fresh.filter((finding) => finding.tier !== 'unusual');
  const unusual = fresh.length - serious.length;
  const toasts: Toast[] = serious.slice(0, MAX_INDIVIDUAL).map((finding) => ({
    title: `SYNTRAC: ${finding.tier} finding`,
    body: cut(`${finding.title}. ${finding.why}`),
  }));
  const more = serious.length - MAX_INDIVIDUAL;
  if (more > 0) {
    toasts.push({ title: `SYNTRAC: ${more} more suspicious ${more === 1 ? 'finding' : 'findings'}`, body: SEE_THEM });
  }
  if (unusual > 0) {
    toasts.push({ title: `SYNTRAC: ${unusual} ${unusual === 1 ? 'finding' : 'findings'} to look at`, body: SEE_THEM });
  }
  return { toasts, seen: next };
}
