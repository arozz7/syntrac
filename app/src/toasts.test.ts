import { describe, expect, it } from 'vitest';
import type { FindingView } from './api/types';
import { planToasts } from './toasts';

function finding(id: number, tier: FindingView['tier'], overrides: Partial<FindingView> = {}): FindingView {
  return {
    id,
    kind: 'new_program',
    subject: `s${id}`,
    subjectLabel: `s${id}`,
    tier,
    title: `Finding ${id}`,
    why: `Why ${id}.`,
    evidence: [],
    status: 'open',
    firstAtMs: 1,
    lastAtMs: 1,
    hits: 1,
    decidedAtMs: null,
    snoozedUntilMs: null,
    folderScope: null,
    ...overrides,
  };
}

describe('planToasts', () => {
  it('shows nothing the first time: what is already open is not news', () => {
    const plan = planToasts([finding(1, 'suspicious'), finding(2, 'unusual')], null);

    expect(plan.toasts).toEqual([]);
    expect([...plan.seen].sort()).toEqual([1, 2]);
  });

  it('raises one toast for each new suspicious or confirmed finding, with its title and why', () => {
    const plan = planToasts([finding(1, 'suspicious'), finding(2, 'confirmed', { title: 'Hash match' })], new Set());

    expect(plan.toasts).toHaveLength(2);
    expect(plan.toasts[0]).toEqual({ title: 'SYNTRAC: suspicious finding', body: 'Finding 1. Why 1.' });
    expect(plan.toasts[1]?.title).toBe('SYNTRAC: confirmed finding');
  });

  it('raises one summary for the unusual ones, never one each', () => {
    const plan = planToasts([finding(1, 'unusual'), finding(2, 'unusual'), finding(3, 'unusual')], new Set());

    expect(plan.toasts).toEqual([
      { title: 'SYNTRAC: 3 findings to look at', body: 'Open the Findings tab to see them.' },
    ]);
  });

  it('does not raise what was already seen, and remembers what is new', () => {
    const plan = planToasts([finding(1, 'suspicious'), finding(2, 'suspicious')], new Set([1]));

    expect(plan.toasts).toHaveLength(1);
    expect(plan.toasts[0]?.body).toContain('Finding 2');
    expect([...plan.seen].sort()).toEqual([1, 2]);
  });

  it('shows at most three suspicious toasts and says how many more there are', () => {
    const many = [1, 2, 3, 4, 5].map((id) => finding(id, 'suspicious'));

    const plan = planToasts(many, new Set());

    expect(plan.toasts).toHaveLength(4);
    expect(plan.toasts[3]).toEqual({
      title: 'SYNTRAC: 2 more suspicious findings',
      body: 'Open the Findings tab to see them.',
    });
  });

  it('only open findings count, and a long text is cut', () => {
    const decided = finding(1, 'suspicious', { status: 'expected' });
    const long = finding(2, 'suspicious', { why: 'x'.repeat(500) });

    const plan = planToasts([decided, long], new Set());

    expect(plan.toasts).toHaveLength(1);
    expect(plan.toasts[0]?.body.length).toBeLessThanOrEqual(200);
  });

  it('forgets findings that are no longer open, so one that comes back is news again', () => {
    const plan = planToasts([], new Set([1, 2]));

    expect([...plan.seen]).toEqual([]);
  });
});
